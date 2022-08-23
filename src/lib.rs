#![cfg_attr(not(test), no_std)]
//! Implement a somewhat convenient and somewhat efficient way to perform RPC
//! in an embedded context.
//!
//! The approach is inspired by Go's channels, with the restriction that
//! there is a clear separation into a requester and a responder.
//!
//! Requests may be canceled, which the responder should honour on a
//! best-effort basis.
//!
//! ### Example use cases
//! - USB device interrupt handler performs low-level protocol details, hands off
//!   commands from the host to higher-level logic running in the idle thread.
//!   This higher-level logic need only understand clearly typed commands, as
//!   moduled by variants of a given `Request` enum.
//! - `trussed` crypto service, responding to crypto request from apps across
//!   TrustZone for Cortex-M secure/non-secure boundaries.
//! - Request to blink a few lights and reply on button press
//!
//!
//! ### Approach
//! It is assumed that all requests fit in a single `Request` enum, and that
//! all responses fit in single `Response` enum. The macro `interchange!`
//! allocates a static buffer in which either response or request fit, and
//! handles synchronization.
//!
//! An alternative approach would be to use two heapless Queues of length one
//! each for response and requests. The advantage of our construction is to
//! have only one static memory region in use.
//!
//! ```
//! # #![cfg(not(loom))]
//! # use interchange::{State, Interchange, interchange};
//! #[derive(Clone, Debug, PartialEq)]
//! pub enum Request {
//!     This(u8, u32),
//!     That(i64),
//! }
//!
//! #[derive(Clone, Debug, PartialEq)]
//! pub enum Response {
//!     Here(u8, u8, u8),
//!     There(i16),
//! }
//!
//! static INTERCHANGE: Interchange<Request, Response, 1>
//!      = interchange!(Request, Response);
//!
//! let (mut rq, mut rp) = INTERCHANGE.claim().unwrap();
//!
//! assert!(rq.state() == State::Idle);
//!
//! // happy path: no cancelation
//! let request = Request::This(1, 2);
//! assert!(rq.request(&request).is_ok());
//!
//! let request = rp.take_request().unwrap();
//! println!("rp got request: {:?}", &request);
//!
//! let response = Response::There(-1);
//! assert!(!rp.is_canceled());
//! assert!(rp.respond(&response).is_ok());
//!
//! let response = rq.take_response().unwrap();
//! println!("rq got response: {:?}", &response);
//!
//! // early cancelation path
//! assert!(rq.request(&request).is_ok());
//!
//! let request =  rq.cancel().unwrap().unwrap();
//! println!("responder could cancel: {:?}", &request);
//!
//! assert!(rp.take_request().is_none());
//! assert!(State::Idle == rq.state());
//!
//! // late cancelation
//! assert!(rq.request(&request).is_ok());
//! let request = rp.take_request().unwrap();
//!
//! println!("responder could cancel: {:?}", &rq.cancel().unwrap().is_none());
//! assert!(rp.is_canceled());
//! assert!(rp.respond(&response).is_err());
//! assert!(rp.acknowledge_cancel().is_ok());
//! assert!(State::Idle == rq.state());
//!
//! // building into request buffer
//! impl Default for Request {
//!   fn default() -> Self {
//!     Request::That(0)
//!   }
//! }
//!
//! rq.with_request_mut(|r| *r = Request::This(1,2)).unwrap() ;
//! assert!(rq.send_request().is_ok());
//! let request = rp.take_request().unwrap();
//! assert_eq!(request, Request::This(1, 2));
//! println!("rp got request: {:?}", &request);
//!
//! // building into response buffer
//! impl Default for Response {
//!   fn default() -> Self {
//!     Response::There(1)
//!   }
//! }
//!
//! rp.with_response_mut(|r| *r = Response::Here(3,2,1)).unwrap();
//! assert!(rp.send_response().is_ok());
//! let response = rq.take_response().unwrap();
//! assert_eq!(response, Response::Here(3,2,1));
//!
//! ```

use core::fmt::{self, Debug};
use core::sync::atomic::Ordering;

#[cfg(loom)]
use loom::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize},
};

#[cfg(not(loom))]
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize},
};

#[derive(Clone, Copy)]
pub struct Error;

impl Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("The interchange is busy, this operation could not be performed")
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
/// State of the RPC interchange
pub enum State {
    /// The requester may send a new request.
    Idle = 0,
    /// The requester is building a request, using the pre-allocated static data as &mut Request
    BuildingRequest = 1,
    /// The request is pending either processing by responder or cancelation by requester.
    Requested = 2,
    /// The responder is building a response, using the pre-allocated static data as &mut Response
    /// It may opportunitstically be canceled by requester.
    BuildingResponse = 3,
    /// The responder sent a response.
    Responded = 4,

    #[doc(hidden)]
    CancelingRequested = 10,
    #[doc(hidden)]
    CancelingBuildingResponse = 11,
    /// The requester canceled the request. Responder needs to acknowledge to return to `Idle`
    /// state.
    Canceled = 12,
}

impl PartialEq<u8> for State {
    #[inline]
    fn eq(&self, other: &u8) -> bool {
        *self as u8 == *other
    }
}

impl From<u8> for State {
    fn from(byte: u8) -> Self {
        match byte {
            1 => State::BuildingRequest,
            2 => State::Requested,
            3 => State::BuildingResponse,
            4 => State::Responded,

            10 => State::CancelingRequested,
            11 => State::CancelingBuildingResponse,
            12 => State::Canceled,

            _ => State::Idle,
        }
    }
}

// the repr(u8) is necessary so MaybeUninit::zeroized.assume_init() is valid and corresponds to
// None
#[repr(u8)]
enum Message<Q, A> {
    None,
    Request(Q),
    Response(A),
}

impl<Q, A> Message<Q, A>
where
    Q: Clone,
    A: Clone,
{
    fn is_request_state(&self) -> bool {
        matches!(self, Self::Request(_))
    }

    fn is_response_state(&self) -> bool {
        matches!(self, Self::Response(_))
    }

    #[allow(unused)]
    unsafe fn rq(self) -> Q {
        match self {
            Self::Request(request) => request,
            _ => unreachable!(),
        }
    }

    unsafe fn rq_ref(&self) -> &Q {
        match *self {
            Self::Request(ref request) => request,
            _ => unreachable!(),
        }
    }

    unsafe fn rq_mut(&mut self) -> &mut Q {
        match *self {
            Self::Request(ref mut request) => request,
            _ => unreachable!(),
        }
    }

    #[allow(unused)]
    unsafe fn rp(self) -> A {
        match self {
            Self::Response(response) => response,
            _ => unreachable!(),
        }
    }

    unsafe fn rp_ref(&self) -> &A {
        match *self {
            Self::Response(ref response) => response,
            _ => unreachable!(),
        }
    }

    unsafe fn rp_mut(&mut self) -> &mut A {
        match *self {
            Self::Response(ref mut response) => response,
            _ => unreachable!(),
        }
    }

    fn from_rq(rq: &Q) -> Self {
        Self::Request(rq.clone())
    }

    fn from_rp(rp: &A) -> Self {
        Self::Response(rp.clone())
    }
}

/// Channel used for Request/Response mechanism.
///
/// The Channel doesn't implement any mechanism to prevent it from beind [`split()`](Channel::split) twice.
/// It is generally recommended to use [`Interchange`](Interchange) instead, which includes a
/// safe API to "Claim" many channels
///
/// ```
/// # #![cfg(not(loom))]
/// # use interchange::{State, Channel};
/// #[derive(Clone, Debug, PartialEq)]
/// pub enum Request {
///     This(u8, u32),
///     That(i64),
/// }
///
/// #[derive(Clone, Debug, PartialEq)]
/// pub enum Response {
///     Here(u8, u8, u8),
///     There(i16),
/// }
///
/// static CHANNEL: Channel<Request,Response> = Channel::new();
///
/// let (mut rq, mut rp) = CHANNEL.split().unwrap();
///
/// assert!(rq.state() == State::Idle);
///
/// // happy path: no cancelation
/// let request = Request::This(1, 2);
/// assert!(rq.request(&request).is_ok());
///
/// let request = rp.take_request().unwrap();
/// println!("rp got request: {:?}", &request);
///
/// let response = Response::There(-1);
/// assert!(!rp.is_canceled());
/// assert!(rp.respond(&response).is_ok());
///
/// let response = rq.take_response().unwrap();
/// println!("rq got response: {:?}", &response);
///
/// // early cancelation path
/// assert!(rq.request(&request).is_ok());
///
/// let request =  rq.cancel().unwrap().unwrap();
/// println!("responder could cancel: {:?}", &request);
///
/// assert!(rp.take_request().is_none());
/// assert!(State::Idle == rq.state());
///
/// // late cancelation
/// assert!(rq.request(&request).is_ok());
/// let request = rp.take_request().unwrap();
///
/// println!("responder could cancel: {:?}", &rq.cancel().unwrap().is_none());
/// assert!(rp.is_canceled());
/// assert!(rp.respond(&response).is_err());
/// assert!(rp.acknowledge_cancel().is_ok());
/// assert!(State::Idle == rq.state());
///
/// // building into request buffer
/// impl Default for Request {
///   fn default() -> Self {
///     Request::That(0)
///   }
/// }
///
/// rq.with_request_mut(|r| *r = Request::This(1,2)).unwrap() ;
/// assert!(rq.send_request().is_ok());
/// let request = rp.take_request().unwrap();
/// assert_eq!(request, Request::This(1, 2));
/// println!("rp got request: {:?}", &request);
///
/// // building into response buffer
/// impl Default for Response {
///   fn default() -> Self {
///     Response::There(1)
///   }
/// }
///
/// rp.with_response_mut(|r| *r = Response::Here(3,2,1)).unwrap();
/// assert!(rp.send_response().is_ok());
/// let response = rq.take_response().unwrap();
/// assert_eq!(response, Response::Here(3,2,1));
///
/// ```
pub struct Channel<Q, A> {
    data: UnsafeCell<Message<Q, A>>,
    state: AtomicU8,
    requester_claimed: AtomicBool,
    responder_claimed: AtomicBool,
}

impl<Q, A> Channel<Q, A>
where
    Q: Clone,
    A: Clone,
{
    // Loom's atomics are not const :/
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            data: UnsafeCell::new(Message::None),
            state: AtomicU8::new(0),
            requester_claimed: AtomicBool::new(false),
            responder_claimed: AtomicBool::new(false),
        }
    }

    #[cfg(loom)]
    pub fn new() -> Self {
        Self {
            data: UnsafeCell::new(Message::None),
            state: AtomicU8::new(0),
            requester_claimed: AtomicBool::new(false),
            responder_claimed: AtomicBool::new(false),
        }
    }

    /// Obtain the requester end of the channel if it hasn't been taken yet
    pub fn requester(&self) -> Option<Requester<'_, Q, A>> {
        if self
            .requester_claimed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(Requester { channel: self })
        } else {
            None
        }
    }

    /// Obtain the responder end of the channel if it hasn't been taken yet
    pub fn responder(&self) -> Option<Responder<'_, Q, A>> {
        if self
            .responder_claimed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(Responder { channel: self })
        } else {
            None
        }
    }

    /// Obtain both the requester and responder ends of the channel
    pub fn split(&self) -> Option<(Requester<'_, Q, A>, Responder<'_, Q, A>)> {
        Some((self.requester()?, self.responder()?))
    }
}

impl<Q, A> Default for Channel<Q, A>
where
    Q: Clone,
    A: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Requester end of a channel
///
/// For a `static` [`Channel`](Channel) or [`Interchange`](Interchange),
/// the requester uses a `'static` lifetime parameter
pub struct Requester<'i, Q, A> {
    channel: &'i Channel<Q, A>,
}

impl<'i, Q, A> Drop for Requester<'i, Q, A> {
    fn drop(&mut self) {
        self.channel
            .requester_claimed
            .store(false, Ordering::Release);
    }
}

impl<'i, Q, A> Requester<'i, Q, A>
where
    Q: Clone,
    A: Clone,
{
    #[inline]
    fn transition(&self, from: State, to: State) -> bool {
        self.channel
            .state
            .compare_exchange(from as u8, to as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    #[cfg(not(loom))]
    unsafe fn data(&self) -> &Message<Q, A> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn data_mut(&mut self) -> &mut Message<Q, A> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Q, A>) -> R) -> R {
        f(&*self.channel.data.get())
    }

    #[cfg(not(loom))]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Q, A>) -> R) -> R {
        f(&mut *self.channel.data.get())
    }

    #[cfg(loom)]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Q, A>) -> R) -> R {
        self.channel.data.with(|i| f(&*i))
    }

    #[cfg(loom)]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Q, A>) -> R) -> R {
        self.channel.data.with_mut(|i| f(&mut *i))
    }

    #[inline]
    /// Current state of the channel.
    ///
    /// Informational only!
    ///
    /// The responder may change this state between calls,
    /// internally atomics ensure correctness.
    pub fn state(&self) -> State {
        State::from(self.channel.state.load(Ordering::Acquire))
    }

    /// Send a request to the responder.
    ///
    /// If efficiency is a concern, or requests need multiple steps to
    /// construct, use `request_mut` and `send_request.
    ///
    /// If the RPC state is `Idle`, this always succeeds, else calling
    /// is a logic error and the request is returned.
    pub fn request(&mut self, request: &Q) -> Result<(), Error> {
        if State::Idle == self.channel.state.load(Ordering::Acquire) {
            unsafe {
                self.with_data_mut(|i| *i = Message::from_rq(request));
            }
            self.channel
                .state
                .store(State::Requested as u8, Ordering::Release);
            Ok(())
        } else {
            Err(Error)
        }
    }

    /// Attempt to cancel a request.
    ///
    /// If the responder has not taken the request yet, this succeeds and returns
    /// the request.
    ///
    /// If the responder has taken the request (is processing), we succeed and return None.
    ///
    /// In other cases (`Idle` or `Reponsed`) there is nothing to cancel and we fail.
    pub fn cancel(&mut self) -> Result<Option<Q>, Error> {
        // we canceled before the responder was even aware of the request.
        if self.transition(State::Requested, State::CancelingRequested) {
            self.channel
                .state
                .store(State::Idle as u8, Ordering::Release);
            return Ok(Some(unsafe { self.with_data(|i| i.rq_ref().clone()) }));
        }

        // we canceled after the responder took the request, but before they answered.
        if self.transition(State::BuildingResponse, State::CancelingRequested) {
            // this may not yet be None in case the responder switched state to
            // BuildingResponse but did not take out the request yet.
            // assert!(self.data.is_none());
            self.channel
                .state
                .store(State::Canceled as u8, Ordering::Release);
            return Ok(None);
        }

        Err(Error)
    }

    /// If there is a response waiting, obtain a reference to it
    ///
    /// This may be called multiple times.
    // Safety: We cannot test this with loom efficiently, but given that `with_response` is tested,
    // this is likely correct
    #[cfg(not(loom))]
    pub fn response(&self) -> Result<&A, Error> {
        if self.transition(State::Responded, State::Responded) {
            Ok(unsafe { self.data().rp_ref() })
        } else {
            Err(Error)
        }
    }

    /// If there is a request waiting, perform an operation with a reference to it
    ///
    /// This may be called multiple times.
    pub fn with_response<R>(&self, f: impl FnOnce(&A) -> R) -> Result<R, Error> {
        if self.transition(State::Responded, State::Responded) {
            Ok(unsafe { self.with_data(|i| f(i.rp_ref())) })
        } else {
            Err(Error)
        }
    }

    /// Look for a response.
    /// If the responder has sent a response, we return it.
    ///
    /// This may be called only once as it move the state to Idle.
    /// If you need copies, clone the request.
    // It is a logic error to call this method if we're Idle or Canceled, but
    // it seems unnecessary to model this.
    pub fn take_response(&mut self) -> Option<A> {
        if self.transition(State::Responded, State::Idle) {
            Some(unsafe { self.with_data(|i| i.rp_ref().clone()) })
        } else {
            None
        }
    }
}

impl<'i, Q, A> Requester<'i, Q, A>
where
    Q: Clone + Default,
    A: Clone,
{
    /// Initialize a request with its default values and mutates it with `f`
    ///
    /// This is usefull to build large structures in-place
    pub fn with_request_mut<R>(&mut self, f: impl FnOnce(&mut Q) -> R) -> Result<R, Error> {
        if self.transition(State::Idle, State::BuildingRequest)
            || self.transition(State::BuildingRequest, State::BuildingRequest)
        {
            let res = unsafe {
                self.with_data_mut(|i| {
                    if !i.is_request_state() {
                        *i = Message::from_rq(&Q::default());
                    }
                    f(i.rq_mut())
                })
            };
            Ok(res)
        } else {
            Err(Error)
        }
    }

    /// Initialize a request with its default values and and return a mutable reference to it
    ///
    /// This is usefull to build large structures in-place
    // Safety: We cannot test this with loom efficiently, but given that `with_request_mut` is tested,
    // this is likely correct
    #[cfg(not(loom))]
    pub fn request_mut(&mut self) -> Result<&mut Q, Error> {
        if self.transition(State::Idle, State::BuildingRequest)
            || self.transition(State::BuildingRequest, State::BuildingRequest)
        {
            unsafe {
                self.with_data_mut(|i| {
                    if !i.is_request_state() {
                        *i = Message::from_rq(&Q::default());
                    }
                })
            }
            Ok(unsafe { self.data_mut().rq_mut() })
        } else {
            Err(Error)
        }
    }

    /// Send a request that was already placed in the channel using `request_mut` or
    /// `with_request_mut`.
    pub fn send_request(&mut self) -> Result<(), Error> {
        if State::BuildingRequest == self.channel.state.load(Ordering::Acquire)
            && self.transition(State::BuildingRequest, State::Requested)
        {
            Ok(())
        } else {
            // logic error
            Err(Error)
        }
    }
}

/// Responder end of a channel
///
/// For a `static` [`Channel`](Channel) or [`Interchange`](Interchange),
/// the responder uses a `'static` lifetime parameter
pub struct Responder<'i, Q, A> {
    channel: &'i Channel<Q, A>,
}

impl<'i, Q, A> Drop for Responder<'i, Q, A> {
    fn drop(&mut self) {
        self.channel
            .responder_claimed
            .store(false, Ordering::Release);
    }
}

impl<'i, Q, A> Responder<'i, Q, A>
where
    Q: Clone,
    A: Clone,
{
    #[inline]
    fn transition(&self, from: State, to: State) -> bool {
        self.channel
            .state
            .compare_exchange(from as u8, to as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    #[cfg(not(loom))]
    unsafe fn data(&self) -> &Message<Q, A> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn data_mut(&mut self) -> &mut Message<Q, A> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Q, A>) -> R) -> R {
        f(&*self.channel.data.get())
    }

    #[cfg(not(loom))]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Q, A>) -> R) -> R {
        f(&mut *self.channel.data.get())
    }

    #[cfg(loom)]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Q, A>) -> R) -> R {
        self.channel.data.with(|i| f(&*i))
    }

    #[cfg(loom)]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Q, A>) -> R) -> R {
        self.channel.data.with_mut(|i| f(&mut *i))
    }

    #[inline]
    /// Current state of the channel.
    ///
    /// Informational only!
    ///
    /// The responder may change this state between calls,
    /// internally atomics ensure correctness.
    pub fn state(&self) -> State {
        State::from(self.channel.state.load(Ordering::Acquire))
    }

    /// If there is a request waiting, perform an operation with a reference to it
    ///
    /// This may be called only once as it move the state to BuildingResponse.
    /// If you need copies, use `take_request`
    pub fn with_request<R>(&self, f: impl FnOnce(&Q) -> R) -> Result<R, Error> {
        if self.transition(State::Requested, State::BuildingResponse) {
            Ok(unsafe { self.with_data(|i| f(i.rq_ref())) })
        } else {
            Err(Error)
        }
    }

    /// If there is a request waiting, obtain a reference to it
    ///
    /// This may be called multiple times.
    // Safety: We cannot test this with loom efficiently, but given that `with_request` is tested,
    // this is likely correct
    #[cfg(not(loom))]
    pub fn request(&self) -> Result<&Q, Error> {
        if self.transition(State::Requested, State::BuildingResponse) {
            Ok(unsafe { self.data().rq_ref() })
        } else {
            Err(Error)
        }
    }

    /// If there is a request waiting, take a reference to it out
    ///
    /// This may be called only once as it move the state to BuildingResponse.
    /// If you need copies, clone the request.
    pub fn take_request(&mut self) -> Option<Q> {
        if self.transition(State::Requested, State::BuildingResponse) {
            Some(unsafe { self.with_data(|i| i.rq_ref().clone()) })
        } else {
            None
        }
    }

    // Check if requester attempted to cancel
    pub fn is_canceled(&self) -> bool {
        self.channel.state.load(Ordering::SeqCst) == State::Canceled as u8
    }

    // Acknowledge a cancel, thereby setting Channel to Idle state again.
    //
    // It is a logic error to call this method if there is no pending cancellation.
    pub fn acknowledge_cancel(&self) -> Result<(), Error> {
        if self
            .channel
            .state
            .compare_exchange(
                State::Canceled as u8,
                State::Idle as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            Ok(())
        } else {
            Err(Error)
        }
    }

    /// Respond to a request.
    ///
    /// If efficiency is a concern, or responses need multiple steps to
    /// construct, use `with_response_mut` or `response_mut` and `send_response`.
    ///
    pub fn respond(&mut self, response: &A) -> Result<(), Error> {
        if State::BuildingResponse == self.channel.state.load(Ordering::Acquire) {
            unsafe {
                self.with_data_mut(|i| *i = Message::from_rp(response));
            }
            self.channel
                .state
                .store(State::Responded as u8, Ordering::Release);
            Ok(())
        } else {
            Err(Error)
        }
    }
}

impl<'i, Q, A> Responder<'i, Q, A>
where
    Q: Clone,
    A: Clone + Default,
{
    /// Initialize a response with its default values and mutates it with `f`
    ///
    /// This is usefull to build large structures in-place
    pub fn with_response_mut<R>(&mut self, f: impl FnOnce(&mut A) -> R) -> Result<R, Error> {
        if self.transition(State::Requested, State::BuildingResponse)
            || self.transition(State::BuildingResponse, State::BuildingResponse)
        {
            let res = unsafe {
                self.with_data_mut(|i| {
                    if !i.is_response_state() {
                        *i = Message::from_rp(&A::default());
                    }
                    f(i.rp_mut())
                })
            };
            Ok(res)
        } else {
            Err(Error)
        }
    }

    /// Initialize a response with its default values and and return a mutable reference to it
    ///
    /// This is usefull to build large structures in-place
    // Safety: We cannot test this with loom efficiently, but given that `with_response_mut` is tested,
    // this is likely correct
    #[cfg(not(loom))]
    pub fn response(&mut self) -> Result<&mut A, Error> {
        if self.transition(State::Requested, State::BuildingResponse)
            || self.transition(State::BuildingResponse, State::BuildingResponse)
        {
            unsafe {
                self.with_data_mut(|i| {
                    if !i.is_response_state() {
                        *i = Message::from_rp(&A::default());
                    }
                })
            }
            Ok(unsafe { self.data_mut().rp_mut() })
        } else {
            Err(Error)
        }
    }

    /// Send a response that was already placed in the channel using `response_mut` or
    /// `with_response_mut`.
    pub fn send_response(&mut self) -> Result<(), Error> {
        if State::BuildingResponse == self.channel.state.load(Ordering::Acquire)
            && self.transition(State::BuildingResponse, State::Responded)
        {
            Ok(())
        } else {
            // logic error
            Err(Error)
        }
    }
}

unsafe impl<'i, Q, A> Send for Responder<'i, Q, A> {}
unsafe impl<'i, Q, A> Send for Requester<'i, Q, A> {}
unsafe impl<Q, A> Send for Channel<Q, A> {}
unsafe impl<Q, A> Sync for Channel<Q, A> {}

/// Set of `N` channels
///
/// Channels can be claimed with [`claim()`](Self::claim)
///
/// ```
/// # #![cfg(not(loom))]
/// # use interchange::*;
/// # #[derive(Clone, Debug, PartialEq)]
/// # pub enum Request {
/// #     This(u8, u32),
/// #     That(i64),
/// # }
/// #
/// # #[derive(Clone, Debug, PartialEq)]
/// # pub enum Response {
/// #     Here(u8, u8, u8),
/// #     There(i16),
/// # }
/// #
/// let interchange: Interchange<_,_,10> = Interchange::new();
///
/// for i in 0..10 {
///     let rq: Requester<'_, Request, Response>;
///     let rp: Responder<'_, Request, Response>;
///     (rq, rp) = interchange.claim().unwrap() ;
/// }
/// ```
pub struct Interchange<Q, A, const N: usize> {
    channels: [Channel<Q, A>; N],
    last_claimed: AtomicUsize,
}

impl<Q, A, const N: usize> Interchange<Q, A, N>
where
    Q: Clone,
    A: Clone,
{
    /// Create a new Interchange
    ///
    /// Due to limitations of current Rust, this method cannot be const. The
    /// [`interchange`](crate::interchange) macro can be used instead.
    pub fn new() -> Self {
        Self {
            channels: core::array::from_fn(|_| Default::default()),
            last_claimed: AtomicUsize::new(0),
        }
    }

    // FIXME: There are many ways to make the new() function const:
    // - Something like [const-zero]: https://docs.rs/const-zero could be used with #[repr(C)] on Message so that 0 = Message::None
    // - Inline const expressions: https://github.com/rust-lang/rust/issues/76001
    #[cfg(not(loom))]
    #[doc(hidden)]
    pub const fn const_new(channels: [Channel<Q, A>; N]) -> Self {
        Self {
            channels,
            last_claimed: AtomicUsize::new(0),
        }
    }

    /// Claim one of the channels of the interchange. Returns None if called more than `N` times.
    pub fn claim(&self) -> Option<(Requester<Q, A>, Responder<Q, A>)> {
        let index = self.last_claimed.fetch_add(1, Ordering::SeqCst);

        for i in (index % N)..N {
            let tmp = self.channels[i].split();
            if tmp.is_some() {
                return tmp;
            }
        }

        for i in 0..(index % N) {
            let tmp = self.channels[i].split();
            if tmp.is_some() {
                return tmp;
            }
        }
        None
    }

    /// Number of clients remaining to claim
    pub fn unclaimed_clients(&self) -> usize {
        N - self.last_claimed.load(core::sync::atomic::Ordering::SeqCst)
    }
}

impl<Q, A, const N: usize> Default for Interchange<Q, A, N>
where
    Q: Clone,
    A: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Create a [Interchange](Interchange) in a `const` context
///
/// ```
/// # #![cfg(not(loom))]
/// # use interchange::*;
/// # #[derive(Clone, Debug, PartialEq)]
/// # pub enum Request {
/// #     This(u8, u32),
/// #     That(i64),
/// # }
/// #
/// # #[derive(Clone, Debug, PartialEq)]
/// # pub enum Response {
/// #     Here(u8, u8, u8),
/// #     There(i16),
/// # }
/// #
/// static INTERCHANGE: Interchange<Request, Response, 10>
///      = interchange!(Request, Response, 10);
///
/// for i in 0..10 {
///     let rq: Requester<'static, Request, Response>;
///     let rp: Responder<'static, Request, Response>;
///     (rq, rp) = INTERCHANGE.claim().unwrap() ;
/// }
/// ```
#[macro_export]
macro_rules! interchange {
    ($REQUEST:ty, $RESPONSE:ty) => {
        $crate::interchange!($REQUEST, $RESPONSE, 1);
    };
    ($REQUEST:ty, $RESPONSE:ty, $N:expr) => {{
        #[allow(clippy::declare_interior_mutable_const)]
        const CHANNEL_NEW: $crate::Channel<$REQUEST, $RESPONSE> = $crate::Channel::new();
        Interchange::const_new([CHANNEL_NEW; $N])
    }};
}
