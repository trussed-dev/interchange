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
//! all responses fit in single `Response` enum. The [`Channel`]() and [`Interchange`]() structs allocate a single buffer in which either Request or Response fit and handle synchronization
//! Both structures have `const` constructors, allowing them to be statically allocated.
//!
//! An alternative approach would be to use two heapless Queues of length one
//! each for response and requests. The advantage of our construction is to
//! have only one static memory region in use.
//!
//! ```
//! # #![cfg(not(loom))]
//! # use interchange::{State, Interchange};
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
//! static INTERCHANGE: Interchange<Request, Response, 1> = Interchange::new();
//!
//! let (mut rq, mut rp) = INTERCHANGE.claim().unwrap();
//!
//! assert!(rq.state() == State::Idle);
//!
//! // happy path: no cancelation
//! let request = Request::This(1, 2);
//! assert!(rq.request(request).is_ok());
//!
//! let request = rp.take_request().unwrap();
//! println!("rp got request: {:?}", request);
//!
//! let response = Response::There(-1);
//! assert!(!rp.is_canceled());
//! assert!(rp.respond(response).is_ok());
//!
//! let response = rq.take_response().unwrap();
//! println!("rq got response: {:?}", response);
//!
//! // early cancelation path
//! assert!(rq.request(request).is_ok());
//!
//! let request =  rq.cancel().unwrap().unwrap();
//! println!("responder could cancel: {:?}", request);
//!
//! assert!(rp.take_request().is_none());
//! assert!(State::Idle == rq.state());
//!
//! // late cancelation
//! assert!(rq.request(request).is_ok());
//! let request = rp.take_request().unwrap();
//!
//! println!("responder could cancel: {:?}", &rq.cancel().unwrap().is_none());
//! assert!(rp.is_canceled());
//! assert!(rp.respond(response).is_err());
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
//! println!("rp got request: {:?}", request);
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
            12 => State::Canceled,
            _ => State::Idle,
        }
    }
}

// the repr(u8) is necessary so MaybeUninit::zeroized.assume_init() is valid and corresponds to
// None
#[repr(u8)]
enum Message<Rq, Rp> {
    None,
    Request(Rq),
    Response(Rp),
}

impl<Rq, Rp> Message<Rq, Rp> {
    fn is_request_state(&self) -> bool {
        matches!(self, Self::Request(_))
    }

    fn is_response_state(&self) -> bool {
        matches!(self, Self::Response(_))
    }

    fn take_rq(&mut self) -> Rq {
        let this = core::mem::replace(self, Message::None);
        match this {
            Message::Request(r) => r,
            _ => unreachable!(),
        }
    }

    fn rq_ref(&self) -> &Rq {
        match *self {
            Self::Request(ref request) => request,
            _ => unreachable!(),
        }
    }

    fn rq_mut(&mut self) -> &mut Rq {
        match *self {
            Self::Request(ref mut request) => request,
            _ => unreachable!(),
        }
    }

    fn take_rp(&mut self) -> Rp {
        let this = core::mem::replace(self, Message::None);
        match this {
            Message::Response(r) => r,
            _ => unreachable!(),
        }
    }

    fn rp_ref(&self) -> &Rp {
        match *self {
            Self::Response(ref response) => response,
            _ => unreachable!(),
        }
    }

    fn rp_mut(&mut self) -> &mut Rp {
        match *self {
            Self::Response(ref mut response) => response,
            _ => unreachable!(),
        }
    }

    fn from_rq(rq: Rq) -> Self {
        Self::Request(rq)
    }

    fn from_rp(rp: Rp) -> Self {
        Self::Response(rp)
    }
}

/// Channel used for Request/Response mechanism.
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
/// assert!(rq.request(request).is_ok());
///
/// let request = rp.take_request().unwrap();
/// println!("rp got request: {:?}", request);
///
/// let response = Response::There(-1);
/// assert!(!rp.is_canceled());
/// assert!(rp.respond(response).is_ok());
///
/// let response = rq.take_response().unwrap();
/// println!("rq got response: {:?}", response);
///
/// // early cancelation path
/// assert!(rq.request(request).is_ok());
///
/// let request =  rq.cancel().unwrap().unwrap();
/// println!("responder could cancel: {:?}", request);
///
/// assert!(rp.take_request().is_none());
/// assert!(State::Idle == rq.state());
///
/// // late cancelation
/// assert!(rq.request(request).is_ok());
/// let request = rp.take_request().unwrap();
///
/// println!("responder could cancel: {:?}", &rq.cancel().unwrap().is_none());
/// assert!(rp.is_canceled());
/// assert!(rp.respond(response).is_err());
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
/// println!("rp got request: {:?}", request);
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
pub struct Channel<Rq, Rp> {
    data: UnsafeCell<Message<Rq, Rp>>,
    state: AtomicU8,
    requester_claimed: AtomicBool,
    responder_claimed: AtomicBool,
}

impl<Rq, Rp> Channel<Rq, Rp> {
    #[cfg(not(loom))]
    const CHANNEL_INIT: Channel<Rq, Rp> = Self::new();

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

    /// Obtain the requester end of the channel if it hasn't been taken yet.
    ///
    /// Can be called again if the previously obtained [`Requester`]() has been dropped
    pub fn requester(&self) -> Option<Requester<'_, Rq, Rp>> {
        if self
            .requester_claimed
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            Some(Requester { channel: self })
        } else {
            None
        }
    }

    /// Obtain the responder end of the channel if it hasn't been taken yet.
    ///
    /// Can be called again if the previously obtained [`Responder`]() has been dropped
    pub fn responder(&self) -> Option<Responder<'_, Rq, Rp>> {
        if self
            .responder_claimed
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            Some(Responder { channel: self })
        } else {
            None
        }
    }

    /// Obtain both the requester and responder ends of the channel.
    ///
    /// Can be called again if the previously obtained [`Responder`]() and [`Requester`]() have been dropped
    pub fn split(&self) -> Option<(Requester<'_, Rq, Rp>, Responder<'_, Rq, Rp>)> {
        Some((self.requester()?, self.responder()?))
    }

    fn transition(&self, from: State, to: State) -> bool {
        self.state
            .compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }
}

impl<Rq, Rp> Default for Channel<Rq, Rp> {
    fn default() -> Self {
        Self::new()
    }
}

/// Requester end of a channel
///
/// For a `static` [`Channel`]() or [`Interchange`](),
/// the requester uses a `'static` lifetime parameter
pub struct Requester<'i, Rq, Rp> {
    channel: &'i Channel<Rq, Rp>,
}

impl<'i, Rq, Rp> Drop for Requester<'i, Rq, Rp> {
    fn drop(&mut self) {
        self.channel
            .requester_claimed
            .store(false, Ordering::Release);
    }
}

impl<'i, Rq, Rp> Requester<'i, Rq, Rp> {
    pub fn channel(&self) -> &'i Channel<Rq, Rp> {
        self.channel
    }

    #[cfg(not(loom))]
    unsafe fn data(&self) -> &Message<Rq, Rp> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn data_mut(&mut self) -> &mut Message<Rq, Rp> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Rq, Rp>) -> R) -> R {
        f(&*self.channel.data.get())
    }

    #[cfg(not(loom))]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Rq, Rp>) -> R) -> R {
        f(&mut *self.channel.data.get())
    }

    #[cfg(loom)]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Rq, Rp>) -> R) -> R {
        self.channel.data.with(|i| f(&*i))
    }

    #[cfg(loom)]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Rq, Rp>) -> R) -> R {
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
    pub fn request(&mut self, request: Rq) -> Result<(), Error> {
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
    pub fn cancel(&mut self) -> Result<Option<Rq>, Error> {
        if self
            .channel
            .transition(State::BuildingResponse, State::Canceled)
        {
            // we canceled after the responder took the request, but before they answered.
            return Ok(None);
        }

        if self.channel.transition(State::Requested, State::Idle) {
            // we canceled before the responder was even aware of the request.
            return Ok(Some(unsafe { self.with_data_mut(|i| i.take_rq()) }));
        }

        Err(Error)
    }

    /// If there is a response waiting, obtain a reference to it
    ///
    /// This may be called multiple times.
    // Safety: We cannot test this with loom efficiently, but given that `with_response` is tested,
    // this is likely correct
    #[cfg(not(loom))]
    pub fn response(&self) -> Result<&Rp, Error> {
        if self.channel.transition(State::Responded, State::Responded) {
            Ok(unsafe { self.data().rp_ref() })
        } else {
            Err(Error)
        }
    }

    /// If there is a request waiting, perform an operation with a reference to it
    ///
    /// This may be called multiple times.
    pub fn with_response<R>(&self, f: impl FnOnce(&Rp) -> R) -> Result<R, Error> {
        if self.channel.transition(State::Responded, State::Responded) {
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
    pub fn take_response(&mut self) -> Option<Rp> {
        if self.channel.transition(State::Responded, State::Idle) {
            Some(unsafe { self.with_data_mut(|i| i.take_rp()) })
        } else {
            None
        }
    }
}

impl<'i, Rq, Rp> Requester<'i, Rq, Rp>
where
    Rq: Default,
{
    /// Initialize a request with its default values and mutates it with `f`
    ///
    /// This is usefull to build large structures in-place
    pub fn with_request_mut<R>(&mut self, f: impl FnOnce(&mut Rq) -> R) -> Result<R, Error> {
        if self.channel.transition(State::Idle, State::BuildingRequest)
            || self
                .channel
                .transition(State::BuildingRequest, State::BuildingRequest)
        {
            let res = unsafe {
                self.with_data_mut(|i| {
                    if !i.is_request_state() {
                        *i = Message::from_rq(Rq::default());
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
    pub fn request_mut(&mut self) -> Result<&mut Rq, Error> {
        if self.channel.transition(State::Idle, State::BuildingRequest)
            || self
                .channel
                .transition(State::BuildingRequest, State::BuildingRequest)
        {
            unsafe {
                self.with_data_mut(|i| {
                    if !i.is_request_state() {
                        *i = Message::from_rq(Rq::default());
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
            && self
                .channel
                .transition(State::BuildingRequest, State::Requested)
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
/// For a `static` [`Channel`]() or [`Interchange`](),
/// the responder uses a `'static` lifetime parameter
pub struct Responder<'i, Rq, Rp> {
    channel: &'i Channel<Rq, Rp>,
}

impl<'i, Rq, Rp> Drop for Responder<'i, Rq, Rp> {
    fn drop(&mut self) {
        self.channel
            .responder_claimed
            .store(false, Ordering::Release);
    }
}

impl<'i, Rq, Rp> Responder<'i, Rq, Rp> {
    pub fn channel(&self) -> &'i Channel<Rq, Rp> {
        self.channel
    }

    #[cfg(not(loom))]
    unsafe fn data(&self) -> &Message<Rq, Rp> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn data_mut(&mut self) -> &mut Message<Rq, Rp> {
        &mut *self.channel.data.get()
    }

    #[cfg(not(loom))]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Rq, Rp>) -> R) -> R {
        f(&*self.channel.data.get())
    }

    #[cfg(not(loom))]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Rq, Rp>) -> R) -> R {
        f(&mut *self.channel.data.get())
    }

    #[cfg(loom)]
    unsafe fn with_data<R>(&self, f: impl FnOnce(&Message<Rq, Rp>) -> R) -> R {
        self.channel.data.with(|i| f(&*i))
    }

    #[cfg(loom)]
    unsafe fn with_data_mut<R>(&mut self, f: impl FnOnce(&mut Message<Rq, Rp>) -> R) -> R {
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
    pub fn with_request<R>(&self, f: impl FnOnce(&Rq) -> R) -> Result<R, Error> {
        if self
            .channel
            .transition(State::Requested, State::BuildingResponse)
        {
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
    pub fn request(&self) -> Result<&Rq, Error> {
        if self
            .channel
            .transition(State::Requested, State::BuildingResponse)
        {
            Ok(unsafe { self.data().rq_ref() })
        } else {
            Err(Error)
        }
    }

    /// If there is a request waiting, take a reference to it out
    ///
    /// This may be called only once as it move the state to BuildingResponse.
    /// If you need copies, clone the request.
    pub fn take_request(&mut self) -> Option<Rq> {
        if self
            .channel
            .transition(State::Requested, State::BuildingResponse)
        {
            Some(unsafe { self.with_data_mut(|i| i.take_rq()) })
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
        if self.channel.transition(State::Canceled, State::Idle) {
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
    pub fn respond(&mut self, response: Rp) -> Result<(), Error> {
        if State::BuildingResponse == self.channel.state.load(Ordering::Acquire) {
            unsafe {
                self.with_data_mut(|i| *i = Message::from_rp(response));
            }
            if self
                .channel
                .transition(State::BuildingResponse, State::Responded)
            {
                Ok(())
            } else {
                Err(Error)
            }
        } else {
            Err(Error)
        }
    }
}

impl<'i, Rq, Rp> Responder<'i, Rq, Rp>
where
    Rp: Default,
{
    /// Initialize a response with its default values and mutates it with `f`
    ///
    /// This is usefull to build large structures in-place
    pub fn with_response_mut<R>(&mut self, f: impl FnOnce(&mut Rp) -> R) -> Result<R, Error> {
        if self
            .channel
            .transition(State::Requested, State::BuildingResponse)
            || self
                .channel
                .transition(State::BuildingResponse, State::BuildingResponse)
        {
            let res = unsafe {
                self.with_data_mut(|i| {
                    if !i.is_response_state() {
                        *i = Message::from_rp(Rp::default());
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
    pub fn response_mut(&mut self) -> Result<&mut Rp, Error> {
        if self
            .channel
            .transition(State::Requested, State::BuildingResponse)
            || self
                .channel
                .transition(State::BuildingResponse, State::BuildingResponse)
        {
            unsafe {
                self.with_data_mut(|i| {
                    if !i.is_response_state() {
                        *i = Message::from_rp(Rp::default());
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
            && self
                .channel
                .transition(State::BuildingResponse, State::Responded)
        {
            Ok(())
        } else {
            // logic error
            Err(Error)
        }
    }
}

// Safety: The channel can be split, which then allows getting sending the Rq and Rp types across threads
// TODO: is the Sync bound really necessary?
unsafe impl<Rq, Rp> Sync for Channel<Rq, Rp>
where
    Rq: Send + Sync,
    Rp: Send + Sync,
{
}

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
/// static interchange: Interchange<Request, Response,10> = Interchange::new();
///
/// for i in 0..10 {
///     let rq: Requester<'_, Request, Response>;
///     let rp: Responder<'_, Request, Response>;
///     (rq, rp) = interchange.claim().unwrap() ;
/// }
/// ```
pub struct Interchange<Rq, Rp, const N: usize> {
    channels: [Channel<Rq, Rp>; N],
    last_claimed: AtomicUsize,
}

impl<Rq, Rp, const N: usize> Interchange<Rq, Rp, N> {
    /// Create a new Interchange
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            channels: [Channel::<Rq, Rp>::CHANNEL_INIT; N],
            last_claimed: AtomicUsize::new(0),
        }
    }

    /// Claim one of the channels of the interchange. Returns None if called more than `N` times.
    pub fn claim(&self) -> Option<(Requester<Rq, Rp>, Responder<Rq, Rp>)> {
        self.as_interchange_ref().claim()
    }

    /// Returns a reference to the interchange with the `N` const-generic removed.
    /// This can avoid the requirement to have `const N: usize` everywhere
    /// ```
    /// # #![cfg(not(loom))]
    /// # use interchange::{State, Interchange, InterchangeRef};
    /// # #[derive(Clone, Debug, PartialEq)]
    /// # pub enum Request {
    /// #     This(u8, u32),
    /// #     That(i64),
    /// # }
    /// # #[derive(Clone, Debug, PartialEq)]
    /// # pub enum Response {
    /// #     Here(u8, u8, u8),
    /// #     There(i16),
    /// # }
    /// static INTERCHANGE_INNER: Interchange<Request, Response, 1> = Interchange::new();
    ///
    /// // The size of the interchange is absent from the type
    /// static INTERCHANGE: InterchangeRef<'static, Request, Response> = INTERCHANGE_INNER.as_interchange_ref();
    ///
    /// let (mut rq, mut rp) = INTERCHANGE.claim().unwrap();
    /// ```
    pub const fn as_interchange_ref(&self) -> InterchangeRef<'_, Rq, Rp> {
        InterchangeRef {
            channels: &self.channels,
            last_claimed: &self.last_claimed,
        }
    }
}

/// Interchange witout the `const N: usize` generic parameter
/// Obtained using [`Interchange::as_interchange_ref`](Interchange::as_interchange_ref)
pub struct InterchangeRef<'alloc, Rq, Rp> {
    channels: &'alloc [Channel<Rq, Rp>],
    last_claimed: &'alloc AtomicUsize,
}
impl<'alloc, Rq, Rp> InterchangeRef<'alloc, Rq, Rp> {
    /// Claim one of the channels of the interchange. Returns None if called more than `N` times.
    pub fn claim(&self) -> Option<(Requester<'alloc, Rq, Rp>, Responder<'alloc, Rq, Rp>)> {
        let index = self.last_claimed.fetch_add(1, Ordering::Relaxed);
        let n = self.channels.len();

        for i in (index % n)..n {
            let tmp = self.channels[i].split();
            if tmp.is_some() {
                return tmp;
            }
        }

        for i in 0..(index % n) {
            let tmp = self.channels[i].split();
            if tmp.is_some() {
                return tmp;
            }
        }
        None
    }
}

#[cfg(not(loom))]
impl<Rq, Rp, const N: usize> Default for Interchange<Rq, Rp, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// ```compile_fail
/// use std::rc::Rc;
/// use interchange::*;
/// #[allow(unconditional_recursion, unused)]
/// fn assert_send<T: Send>() {
///     assert_send::<Channel<Rc<String>, u32>>();
/// }
/// ```
/// ```compile_fail
/// use std::rc::Rc;
/// use interchange::*;
/// #[allow(unconditional_recursion, unused)]
/// fn assert_send<T: Send>() {
///     assert_send::<Requester<Rc<String>, u32>>();
/// }
/// ```
/// ```compile_fail
/// use std::rc::Rc;
/// use interchange::*;
/// #[allow(unconditional_recursion, unused)]
/// fn assert_send<T: Send>() {
///     assert_send::<Responder<Rc<String>, u32>>();
/// }
/// ```
/// ```compile_fail
/// use std::rc::Rc;
/// use interchange::*;
/// #[allow(unconditional_recursion, unused)]
/// fn assert_sync<T: Sync>() {
///     assert_sync::<Channel<Rc<String>, u32>>();
/// }
/// ```
/// ```compile_fail
/// use std::rc::Rc;
/// use interchange::*;
/// #[allow(unconditional_recursion, unused)]
/// fn assert_sync<T: Sync>() {
///     assert_sync::<Requester<Rc<String>, u32>>();
/// }
/// ```
/// ```compile_fail
/// use std::rc::Rc;
/// use interchange::*;
/// #[allow(unconditional_recursion, unused)]
/// fn assert_sync<T: Sync>() {
///     assert_sync::<Responder<Rc<String>, u32>>();
/// }
/// ```
const _ASSERT_COMPILE_FAILS: () = {};

#[cfg(all(not(loom), test))]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq)]
    pub enum Request {
        This(u8, u32),
    }
    #[derive(Clone, Debug, PartialEq)]
    pub enum Response {
        Here(u8, u8, u8),
        There(i16),
    }
    impl Default for Response {
        fn default() -> Self {
            Response::There(1)
        }
    }
    impl Default for Request {
        fn default() -> Self {
            Request::This(0, 0)
        }
    }

    #[test]
    fn interchange() {
        static INTERCHANGE: Interchange<Request, Response, 1> = Interchange::new();
        let (mut rq, mut rp) = INTERCHANGE.claim().unwrap();
        assert_eq!(rq.state(), State::Idle);
        // happy path: no cancelation
        let request = Request::This(1, 2);
        assert!(rq.request(request).is_ok());
        let request = rp.take_request().unwrap();
        println!("rp got request: {request:?}");
        let response = Response::There(-1);
        assert!(!rp.is_canceled());
        assert!(rp.respond(response).is_ok());
        let response = rq.take_response().unwrap();
        println!("rq got response: {response:?}");
        // early cancelation path
        assert!(rq.request(request).is_ok());
        let request = rq.cancel().unwrap().unwrap();
        println!("responder could cancel: {request:?}");
        assert!(rp.take_request().is_none());
        assert_eq!(State::Idle, rq.state());
        // late cancelation
        assert!(rq.request(request).is_ok());
        let request = rp.take_request().unwrap();
        println!(
            "responder could cancel: {:?}",
            &rq.cancel().unwrap().is_none()
        );
        assert_eq!(request, Request::This(1, 2));
        assert!(rp.is_canceled());
        assert!(rp.respond(response).is_err());
        assert!(rp.acknowledge_cancel().is_ok());
        assert_eq!(State::Idle, rq.state());
        // building into request buffer
        rq.with_request_mut(|r| *r = Request::This(1, 2)).unwrap();
        assert!(rq.send_request().is_ok());
        let request = rp.take_request().unwrap();
        assert_eq!(request, Request::This(1, 2));
        println!("rp got request: {request:?}");
        // building into response buffer
        rp.with_response_mut(|r| *r = Response::Here(3, 2, 1))
            .unwrap();
        assert!(rp.send_response().is_ok());
        let response = rq.take_response().unwrap();
        assert_eq!(response, Response::Here(3, 2, 1));
    }

    #[test]
    fn interchange_ref() {
        static INTERCHANGE_INNER: Interchange<Request, Response, 1> = Interchange::new();
        static INTERCHANGE: InterchangeRef<'static, Request, Response> =
            INTERCHANGE_INNER.as_interchange_ref();
        let (mut rq, mut rp) = INTERCHANGE.claim().unwrap();
        assert_eq!(rq.state(), State::Idle);
        // happy path: no cancelation
        let request = Request::This(1, 2);
        assert!(rq.request(request).is_ok());
        let request = rp.take_request().unwrap();
        println!("rp got request: {request:?}");
        let response = Response::There(-1);
        assert!(!rp.is_canceled());
        assert!(rp.respond(response).is_ok());
        let response = rq.take_response().unwrap();
        println!("rq got response: {response:?}");
        // early cancelation path
        assert!(rq.request(request).is_ok());
        let request = rq.cancel().unwrap().unwrap();
        println!("responder could cancel: {request:?}");
        assert!(rp.take_request().is_none());
        assert_eq!(State::Idle, rq.state());
        // late cancelation
        assert!(rq.request(request).is_ok());
        let request = rp.take_request().unwrap();
        println!(
            "responder could cancel: {:?}",
            &rq.cancel().unwrap().is_none()
        );
        assert_eq!(request, Request::This(1, 2));
        assert!(rp.is_canceled());
        assert!(rp.respond(response).is_err());
        assert!(rp.acknowledge_cancel().is_ok());
        assert_eq!(State::Idle, rq.state());
        // building into request buffer
        rq.with_request_mut(|r| *r = Request::This(1, 2)).unwrap();
        assert!(rq.send_request().is_ok());
        let request = rp.take_request().unwrap();
        assert_eq!(request, Request::This(1, 2));
        println!("rp got request: {request:?}");
        // building into response buffer
        rp.with_response_mut(|r| *r = Response::Here(3, 2, 1))
            .unwrap();
        assert!(rp.send_response().is_ok());
        let response = rq.take_response().unwrap();
        assert_eq!(response, Response::Here(3, 2, 1));
    }

    #[allow(unconditional_recursion, clippy::extra_unused_type_parameters, unused)]
    fn assert_send<T: Send>() {
        assert_send::<Channel<String, u32>>();
        assert_send::<Responder<'static, String, u32>>();
        assert_send::<Requester<'static, String, u32>>();
        assert_send::<Channel<&'static mut String, u32>>();
        assert_send::<Responder<'static, &'static mut String, u32>>();
        assert_send::<Requester<'static, &'static mut String, u32>>();
    }
    #[allow(unconditional_recursion, clippy::extra_unused_type_parameters, unused)]
    fn assert_sync<T: Sync>() {
        assert_sync::<Channel<String, u32>>();
        assert_sync::<Channel<String, u32>>();
        assert_sync::<Responder<'static, String, u32>>();
        assert_sync::<Requester<'static, String, u32>>();

        assert_sync::<Channel<&'static mut String, u32>>();
        assert_sync::<Responder<'static, &'static mut String, u32>>();
        assert_sync::<Requester<'static, &'static mut String, u32>>();
    }
}
