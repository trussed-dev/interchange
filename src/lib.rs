#![no_std]
//! Implement a somewhat convenient and somewhat efficient way to perform RPC
//! in an embedded context.
//!
//! The approach is inspired by Go's channels, with the restriction that
//! there is a clear separation into a requester and a responder.
//!
//! Requests may be canceled, which the responder should honour on a
//! best-effort basis.
//!
//! For each pair of `Request` and `Response` types, the macro `interchange!`
//! generates a type that implements the `Interchange` trait.
//!
//! The `Requester` and `Responder` types (to send/cancel requests, and to
//! respond to such demands) are generic with only this one type parameter.
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
//! ```
//! # use interchange::Interchange as _;
//! # use interchange::State;
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
//! interchange::interchange! {
//!     ExampleInterchange: (Request, Response)
//! }
//!
//! let (mut rq, mut rp) = ExampleInterchange::claim().unwrap();
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
//! let request_mut = rq.request_mut().unwrap();
//! *request_mut = Request::This(1, 2);
//! assert!(rq.send_request().is_ok());
//! let request = rp.take_request().unwrap();
//! println!("rp got request: {:?}", &request);
//!
//! // building into response buffer
//! impl Default for Response {
//!   fn default() -> Self {
//!     Response::There(1)
//!   }
//! }
//!
//! let response_mut = rp.response_mut().unwrap();
//! *response_mut = Response::Here(3,2,1);
//! assert!(rp.send_response().is_ok());
//! let response = rq.take_response().unwrap();
//! println!("rq got response: {:?}", &response);
//!
//! ```
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
//! ### Safety
//! It is possible that this implementation is currently not sound. To be determined!
//!
//! Due to the macro construction, certain implementation details are more public
//! than one would hope for: the macro needs to run in the code of users of this
//! library. We take a somewhat Pythonic "we're all adults here" approach, in that
//! the user is expected to only use the publicly documented API (the ideally private
//! details are hidden from documentation).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, Ordering};

mod macros;
// pub mod scratch;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq)]
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

/// Do NOT implement this yourself! Use the macro `interchange!`.
///
/// Also, DO NOT use the doc(hidden) methods, if the public API in Requester
/// and Responder are not sufficient for your needs, this means the abstraction
/// is not good enough, and must be fixed here, in `interchange`.
///
/// At compile time, the client capacity is set by using the appropriate call
/// to the `interchange!` macro. The application can then repeatedly call `claim`
/// to obtain these clients.
pub trait Interchange: Sized {
    const CLIENT_CAPACITY: usize;
    type REQUEST: Clone;
    type RESPONSE: Clone;
    /// This is the constructor for a `(Requester, Responder)` pair.
    ///
    /// Returns singleton static instances until all that were allocated are
    /// used up, thereafter, `None` is returned.
    fn claim() -> Option<(Requester<Self>, Responder<Self>)>;

    /// Method for debugging: how many allocated clients have not been claimed.
    fn unclaimed_clients() -> usize;

    /// Method purely for testing - do not use in production
    ///
    /// Rationale: In production, interchanges are supposed to be set up
    /// as global singletons during intialization. In testing however, multiple
    /// test cases are run serially; without this reset, such tests would need
    /// to allocate an extremely large amount of clients.
    ///
    /// It does not work to put this behind a feature flag, as macro expansion
    /// happens at call site and can't see the feature.
    unsafe fn reset_claims();

    #[doc(hidden)]
    fn is_request_state(&self) -> bool;
    #[doc(hidden)]
    fn is_response_state(&self) -> bool;
    #[doc(hidden)]
    unsafe fn rq_ref(&self) -> &Self::REQUEST;
    #[doc(hidden)]
    unsafe fn rp_ref(&self) -> &Self::RESPONSE;
    #[doc(hidden)]
    unsafe fn rq_mut(&mut self) -> &mut Self::REQUEST;
    #[doc(hidden)]
    unsafe fn rp_mut(&mut self) -> &mut Self::RESPONSE;
    #[doc(hidden)]
    fn from_rq(rq: &Self::REQUEST) -> Self;
    #[doc(hidden)]
    fn from_rp(rp: &Self::RESPONSE) -> Self;
    #[doc(hidden)]
    unsafe fn rq(self) -> Self::REQUEST;
    #[doc(hidden)]
    unsafe fn rp(self) -> Self::RESPONSE;
}

/// Requesting end of the RPC interchange.
///
/// The owner of this end initiates RPC by sending a request.
/// It must then either wait until the responder end responds, upon which
/// it can send a new request again. It does so by periodically checking
/// whether `take_response` is Some. Or it can attempt to cancel,
/// which the responder may or may not honour. For details, see the
/// `cancel` method.
pub struct Requester<I: 'static + Interchange> {
    // todo: get rid of this publicity
    #[doc(hidden)]
    pub interchange: &'static UnsafeCell<I>,
    #[doc(hidden)]
    pub state: &'static AtomicU8,
}

unsafe impl<I: Interchange> Send for Requester<I> {}

/// Processing end of the RPC interchange.
///
/// The owner of this end must eventually reply to any requests made to it.
/// In case there is a cancelation of the request, this must be acknowledged instead.
pub struct Responder<I: 'static + Interchange> {
    #[doc(hidden)]
    pub interchange: &'static UnsafeCell<I>,
    #[doc(hidden)]
    pub state: &'static AtomicU8,
}

unsafe impl<I: Interchange> Send for Responder<I> {}

impl<I: Interchange> Requester<I> {
    #[inline]
    fn transition(&self, from: State, to: State) -> bool {
        self.state
            .compare_exchange(from as u8, to as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    unsafe fn interchange(&self) -> &'static I {
        &*self.interchange.get()
    }

    unsafe fn interchange_mut(&mut self) -> &'static mut I {
        &mut *self.interchange.get()
    }

    #[inline]
    /// Current state of the interchange.
    ///
    /// Informational only!
    ///
    /// The responder may change this state between calls,
    /// internally atomics ensure correctness.
    pub fn state(&self) -> State {
        State::from(self.state.load(Ordering::Acquire))
    }

    /// Send a request to the responder.
    ///
    /// If efficiency is a concern, or requests need multiple steps to
    /// construct, use `request_mut` and `send_request.
    ///
    /// If the RPC state is `Idle`, this always succeeds, else calling
    /// is a logic error and the request is returned.
    pub fn request(&mut self, request: &I::REQUEST) -> Result<(), ()> {
        if State::Idle == self.state.load(Ordering::Acquire) {
            unsafe {
                *self.interchange_mut() = Interchange::from_rq(request);
            }
            self.state.store(State::Requested as u8, Ordering::Release);
            Ok(())
        } else {
            Err(())
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
    pub fn cancel(&mut self) -> Result<Option<I::REQUEST>, ()> {
        // we canceled before the responder was even aware of the request.
        if self.transition(State::Requested, State::CancelingRequested) {
            self.state.store(State::Idle as u8, Ordering::Release);
            return Ok(Some(unsafe { (*self.interchange()).rq_ref().clone() }));
        }

        // we canceled after the responder took the request, but before they answered.
        if self.transition(State::BuildingResponse, State::CancelingRequested) {
            // this may not yet be None in case the responder switched state to
            // BuildingResponse but did not take out the request yet.
            // assert!(self.interchange.is_none());
            self.state.store(State::Canceled as u8, Ordering::Release);
            return Ok(None);
        }

        Err(())
    }

    /// Look for a response.
    /// If the responder has sent a response, we return it.
    ///
    /// This may be called multiple times.
    // It is a logic error to call this method if we're Idle or Canceled, but
    // it seems unnecessary to model this.
    pub fn response(&mut self) -> Option<&I::RESPONSE> {
        if self.transition(State::Responded, State::Responded) {
            Some(unsafe { (*self.interchange()).rp_ref() })
        } else {
            None
        }
    }

    /// Look for a response.
    /// If the responder has sent a response, we return it.
    ///
    /// This may be called only once as it move the state to Idle.
    /// If you need copies, clone the request.
    // It is a logic error to call this method if we're Idle or Canceled, but
    // it seems unnecessary to model this.
    pub fn take_response(&mut self) -> Option<I::RESPONSE> {
        if self.transition(State::Responded, State::Idle) {
            Some(unsafe { (*self.interchange()).rp_ref().clone() })
        } else {
            None
        }
    }
}

impl<I: Interchange> Requester<I>
where
    I::REQUEST: Default,
{
    /// If the interchange is idle, may build request into the returned value.
    pub fn request_mut(&mut self) -> Option<&mut I::REQUEST> {
        if self.transition(State::Idle, State::BuildingRequest)
            || self.transition(State::BuildingRequest, State::BuildingRequest)
        {
            unsafe {
                if !(*self.interchange()).is_request_state() {
                    *self.interchange_mut() = I::from_rq(&I::REQUEST::default());
                }

                Some((*self.interchange_mut()).rq_mut())
            }
        } else {
            None
        }
    }

    /// Send a request that was already placed in the interchange using `request_mut`.
    pub fn send_request(&mut self) -> Result<(), ()> {
        if State::BuildingRequest == self.state.load(Ordering::Acquire) {
            unsafe {
                *self.interchange_mut() = I::from_rq(self.request_mut().unwrap());
            }
            if self.transition(State::BuildingRequest, State::Requested) {
                Ok(())
            } else {
                Err(())
            }
        } else {
            // logic error
            Err(())
        }
    }
}

impl<I: Interchange> Responder<I> {
    #[inline]
    fn transition(&self, from: State, to: State) -> bool {
        self.state
            .compare_exchange(from as u8, to as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    unsafe fn interchange(&self) -> &'static I {
        &*self.interchange.get()
    }

    unsafe fn interchange_mut(&mut self) -> &'static mut I {
        &mut *self.interchange.get()
    }

    #[inline]
    /// Current state of the interchange.
    ///
    /// Informational only!
    ///
    /// The responder may change this state between calls,
    /// internally atomics ensure correctness.
    pub fn state(&self) -> State {
        State::from(self.state.load(Ordering::Acquire))
    }

    /// If there is a request waiting, take a reference to it out
    ///
    /// This may be called only once as it move the state to BuildingResponse.
    /// If you need copies, clone the request.
    pub fn request(&mut self) -> Option<&I::REQUEST> {
        if self.transition(State::Requested, State::Requested) {
            Some(unsafe { (*self.interchange()).rq_ref() })
        } else {
            None
        }
    }

    /// If there is a request waiting, take a reference to it out
    ///
    /// This may be called only once as it move the state to BuildingResponse.
    /// If you need copies, clone the request.
    pub fn take_request(&mut self) -> Option<I::REQUEST> {
        if self.transition(State::Requested, State::BuildingResponse) {
            Some(unsafe { (self.interchange()).rq_ref().clone() })
        } else {
            None
        }
    }

    // Check if requester attempted to cancel
    pub fn is_canceled(&self) -> bool {
        self.state.load(Ordering::SeqCst) == State::Canceled as u8
    }

    // Acknowledge a cancel, thereby setting Interchange to Idle state again.
    //
    // It is a logic error to call this method if there is no pending cancellation.
    pub fn acknowledge_cancel(&self) -> Result<(), ()> {
        if self
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
            Err(())
        }
    }

    /// Respond to a request.
    ///
    /// If efficiency is a concern, or responses need multiple steps to
    /// construct, use `response_mut` and `send_response.
    ///
    pub fn respond(&mut self, response: &I::RESPONSE) -> Result<(), ()> {
        if State::BuildingResponse == self.state.load(Ordering::Acquire) {
            unsafe {
                *self.interchange_mut() = I::from_rp(response);
            }
            self.state.store(State::Responded as u8, Ordering::Release);
            Ok(())
        } else {
            Err(())
        }
    }
}

impl<I: Interchange> Responder<I>
where
    I::RESPONSE: Default,
{
    /// If there is a request waiting that no longer needs to
    /// be accessed, may build response into the returned value.
    pub fn response_mut(&mut self) -> Option<&mut I::RESPONSE> {
        if self.transition(State::Requested, State::BuildingResponse)
            || self.transition(State::BuildingResponse, State::BuildingResponse)
        {
            unsafe {
                if !(*self.interchange()).is_response_state() {
                    *self.interchange_mut() = I::from_rp(&I::RESPONSE::default());
                }

                Some((*self.interchange_mut()).rp_mut())
            }
        } else {
            None
        }
    }

    /// Send a response that was already placed in the interchange using `response_mut`.
    pub fn send_response(&mut self) -> Result<(), ()> {
        if State::BuildingResponse == self.state.load(Ordering::Acquire) {
            unsafe {
                *self.interchange_mut() = I::from_rp(self.response_mut().unwrap());
            }
            if self.transition(State::BuildingResponse, State::Responded) {
                Ok(())
            } else {
                Err(())
            }
        } else {
            // logic error
            Err(())
        }
    }
}
