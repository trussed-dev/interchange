/// Use this macro to generate a pair of RPC pipes for any pair
/// of Request/Response enums you wish to implement.
///
/// ```
/// use interchange::Interchange as _;
/// use interchange::interchange;
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
/// interchange::interchange! {
///     ExampleInterchange: (Request, Response)
/// }
/// ```
///
/// # Note
/// The syntax to setup multiple copies of a given interchange (for instance,
/// we use this in `trussed` for multi-client) is horrible. Please let the
/// authers know if there's a better way, than the current
/// `interchange!(Name: (Request, Response), 3, [None, None, None])` etc.
#[macro_export]
macro_rules! interchange {
    ($Name:ident: ($REQUEST:ty, $RESPONSE:ty)) => {
        $crate::interchange!($Name: ($REQUEST, $RESPONSE, 1));
    };

    ($Name:ident: ($REQUEST:ty, $RESPONSE:ty, $N:expr)) => {

        // TODO: figure out how to implement, e.g., Clone iff REQUEST
        // and RESPONSE are clone (do not introduce Clone, Debug, etc. trait bounds).
        #[derive(Clone, Debug, PartialEq)]
        pub enum $Name {
            Request($REQUEST),
            Response($RESPONSE),
            None,
        }

        impl $Name {
            fn split(i: usize) -> ($crate::Requester<Self>, $crate::Responder<Self>) {
                use core::sync::atomic::AtomicU8;
                use core::cell::UnsafeCell;

                // TODO(nickray): This turns up in .data section, fix this.

                #[allow(clippy::declare_interior_mutable_const)]
                const INTERCHANGE_NONE: UnsafeCell<$Name> = UnsafeCell::new($Name::None);
                // `mut` is required because UnsafeCell is neither Send nor Sync
                static mut INTERCHANGES: [UnsafeCell<$Name>; $N] = [INTERCHANGE_NONE; $N];
                #[allow(clippy::declare_interior_mutable_const)]
                const ATOMIC_ZERO: AtomicU8 = AtomicU8::new(0);
                static STATES: [AtomicU8; $N] = [ATOMIC_ZERO; $N];

                (
                    $crate::Requester {
                        interchange: unsafe { &INTERCHANGES[i] },
                        state: &STATES[i],
                    },

                    $crate::Responder {
                        interchange: unsafe { &INTERCHANGES[i] },
                        state: &STATES[i],
                    },
                )
            }

            fn last_claimed() -> &'static core::sync::atomic::AtomicUsize {
                use core::sync::atomic::{AtomicUsize, Ordering};
                static LAST_CLAIMED: AtomicUsize = AtomicUsize::new(0);
                &LAST_CLAIMED
            }

        }

        impl $crate::Interchange for $Name {
            const CLIENT_CAPACITY: usize = $N;

            type REQUEST = $REQUEST;
            type RESPONSE = $RESPONSE;

            unsafe fn reset_claims() {
                // debug!("last claimed was {}",
                //     Self::last_claimed().load( core::sync::atomic::Ordering::SeqCst));
                Self::last_claimed().store(0, core::sync::atomic::Ordering::SeqCst);
                // debug!("last claimed is {}",
                //     Self::last_claimed().load( core::sync::atomic::Ordering::SeqCst));
            }

            fn claim() -> Option<($crate::Requester<Self>, $crate::Responder<Self>)> {
                use core::sync::atomic::{AtomicUsize, Ordering};
                let last_claimed = Self::last_claimed();
                // static LAST_CLAIMED: AtomicUsize = AtomicUsize::new(0);
                let index = last_claimed.fetch_add(1, Ordering::SeqCst);
                if index > $N {
                    None
                } else {
                    Some(Self::split(index))
                }
            }

            fn unclaimed_clients() -> usize {
                Self::CLIENT_CAPACITY - Self::last_claimed().load(core::sync::atomic::Ordering::SeqCst)
            }

            fn is_request_state(&self) -> bool {
                match self {
                    Self::Request(_) => true,
                    _ => false,
                }
            }

            fn is_response_state(&self) -> bool {
                match self {
                    Self::Response(_) => true,
                    _ => false,
                }
            }

            unsafe fn rq(self) -> Self::REQUEST {
                match self {
                    Self::Request(request) => {
                        request
                    }
                    _ => unreachable!(),
                }
            }

            unsafe fn rq_ref(&self) -> &Self::REQUEST {
                match *self {
                    Self::Request(ref request) => {
                        request
                    }
                    _ => unreachable!(),
                }
            }

            unsafe fn rq_mut(&mut self) -> &mut Self::REQUEST {
                match *self {
                    Self::Request(ref mut request) => {
                        request
                    }
                    _ => unreachable!(),
                }
            }

            unsafe fn rp(self) -> Self::RESPONSE {
                match self {
                    Self::Response(response) => {
                        response
                    }
                    _ => unreachable!(),
                }
            }

            unsafe fn rp_ref(&self) -> &Self::RESPONSE {
                match *self {
                    Self::Response(ref response) => {
                        response
                    }
                    _ => unreachable!(),
                }
            }

            unsafe fn rp_mut(&mut self) -> &mut Self::RESPONSE {
                match *self {
                    Self::Response(ref mut response) => {
                        response
                    }
                    _ => unreachable!(),
                }
            }

            fn from_rq(rq: &Self::REQUEST) -> Self {
                Self::Request(rq.clone())
            }

            fn from_rp(rp: &Self::RESPONSE) -> Self {
                Self::Response(rp.clone())
            }

        }

    }
}
