use std::ops::*;
use std::cell::*;
use std::marker::PhantomData;

pub trait FastCacheExpiration<T> {
    fn expire_with<X>(self, expiration: X) -> Self where X: FnOnce(&T) -> bool;
}

pub trait FastCacheGet<'a, T, R> {
    fn get_or_insert_with<F: FnOnce() -> T>(self, f: F) -> R;
}

pub trait FastCacheMaybeGet<'a, T, R> {
    fn maybe_get_or_insert_with<F: FnOnce() -> Option<T>>(self, f: F) -> Option<R>;
}

//
// Option
//

impl<'a, T> FastCacheExpiration<T> for &'a mut Option<T> {
    fn expire_with<X>(self, expiration: X) -> Self
    where X: FnOnce(&T) -> bool {
        if self.as_ref().map(expiration).unwrap_or(false) {
            self.take();
        }

        self
    }
}

impl<'a, T> FastCacheGet<'a, T, &'a T> for &'a mut Option<T> {
    fn get_or_insert_with<F: FnOnce() -> T>(self, f: F) -> &'a T {
        self.get_or_insert_with(f)
    }
}

impl<'a, T> FastCacheMaybeGet<'a, T, &'a T> for &'a mut Option<T> {
    fn maybe_get_or_insert_with<F: FnOnce() -> Option<T>>(self, f: F) -> Option<&'a T> {
        if self.is_none() {
            *self = (f)();
        }

        self.as_ref()
    }
}

//
// Refcell
//

impl<'a, T> FastCacheExpiration<T> for &'a RefCell<Option<T>> {
    fn expire_with<X>(self, expiration: X) -> Self
    where X: FnOnce(&T) -> bool {
        if self.borrow().as_ref().map(expiration).unwrap_or(false) {
            self.borrow_mut().take();
        }

        self
    }
}

impl<'a, T> FastCacheGet<'a, T, Ref<'a, T>> for &'a RefCell<Option<T>> {
    fn get_or_insert_with<F: FnOnce() -> T>(self, f: F) -> Ref<'a, T> {
        if self.borrow().is_none() {
            *self.borrow_mut() = Some(f());
        }

        Ref::map(self.borrow(), |v| v.as_ref().unwrap())
    }
}

impl<'a, T> FastCacheMaybeGet<'a, T, Ref<'a, T>> for &'a RefCell<Option<T>> {
    fn maybe_get_or_insert_with<F: FnOnce() -> Option<T>>(self, f: F) -> Option<Ref<'a, T>> {
        if self.borrow().is_none() {
            *self.borrow_mut() = (f)();
        }

        if self.borrow().is_some() {
            Some(Ref::map(self.borrow(), |v| v.as_ref().unwrap()))
        } else {
            None
        }
    }
}

//
// Implementation
//

pub trait FastCacheAccessor<'a, T, R>: FastCacheExpiration<T> + FastCacheGet<'a, T, R> where Self: Sized {
    fn access<X, F>(self, expiration: X, filler: F) -> CacheAccesor<'a, T, Self, X, F, R> where F: FnOnce() -> T, X: FnOnce(&T) -> bool;
}

pub trait FastCacheMaybeAccessor<'a, T, R>: FastCacheExpiration<T> + FastCacheMaybeGet<'a, T, R> where Self: Sized {
    fn maybe_access<X, F>(self, expiration: X, filler: F) -> MaybeCacheAccesor<'a, T, Self, X, F, R> where F: FnOnce() -> Option<T>, X: FnOnce(&T) -> bool;
}

impl<'a, C, T, R> FastCacheAccessor<'a, T, R> for C where C: FastCacheExpiration<T> + FastCacheGet<'a, T, R> {
    fn access<X, F>(self, expiration: X, filler: F) -> CacheAccesor<'a, T, Self, X, F, R> where F: FnOnce() -> T, X: FnOnce(&T) -> bool {
        CacheAccesor {
            state: CacheState::Unknown(CacheStateUnknown {
                cache: self,
                expiration,
                fill: filler,
                phantom: PhantomData
            }, PhantomData),
            phantom: PhantomData,
        }
    }
}

impl<'a, C, T, R> FastCacheMaybeAccessor<'a, T, R> for C where C: FastCacheExpiration<T> + FastCacheMaybeGet<'a, T, R> {
    fn maybe_access<X, F>(self, expiration: X, filler: F) -> MaybeCacheAccesor<'a, T, Self, X, F, R> where F: FnOnce() -> Option<T>, X: FnOnce(&T) -> bool {
        MaybeCacheAccesor {
            state: MaybeCacheState::Unknown(MaybeCacheStateUnknown {
                cache: self,
                expiration,
                fill: filler,
                phantom: std::marker::PhantomData
            }, PhantomData),
            phantom: PhantomData
        }
    }
}

pub trait Get<R> {
    fn get(&mut self) -> &R;

    fn take(self) -> R;
}

pub struct CacheAccesor<'c, T, C, X, F, R> where F: FnOnce() -> T, X: FnOnce(&T) -> bool, C: FastCacheGet<'c, T, R> + FastCacheExpiration<T> {
    state: CacheState<'c, T, C, X, F, R>,
    phantom: PhantomData<R>
}

impl<'c, T, C, X, F, R> Get<R> for CacheAccesor<'c, T, C, X, F, R> where F: FnOnce() -> T, X: FnOnce(&T) -> bool, C: FastCacheGet<'c, T, R> + FastCacheExpiration<T> {
    fn get(&mut self) -> &R {
        take_mut::take(&mut self.state, |v| v.into_known());

        match &self.state {
            CacheState::Unknown(_, _) => { unsafe { std::hint::unreachable_unchecked() } },
            CacheState::Known(s) => &s.data,
        }
    }

    fn take(self) -> R {
        match self.state.into_known() {
            CacheState::Unknown(_, _) => { unsafe { std::hint::unreachable_unchecked() } },
            CacheState::Known(s) => s.data,
        }
    }
}

pub enum CacheState<'c, T, C, X, F, R> where F: FnOnce() -> T, X: FnOnce(&T) -> bool, C: FastCacheGet<'c, T, R> + FastCacheExpiration<T> {
    Unknown(CacheStateUnknown<'c, T, C, X, F, R>, PhantomData<R>),
    Known(CacheStateKnown<R>)
}

impl<'c, T, C, X, F, R> CacheState<'c, T, C, X, F, R> where F: FnOnce() -> T, X: FnOnce(&T) -> bool, C: FastCacheGet<'c, T, R> + FastCacheExpiration<T> {
    pub fn into_known(self) -> Self {
        match self {
            CacheState::Unknown(state, _) => {
                let ref_val = state.cache
                    .expire_with(state.expiration)
                    .get_or_insert_with(state.fill);
        
                let new_state = CacheStateKnown {
                    data: ref_val
                };

                CacheState::Known(new_state)            
            },
            v => v
        }
    }
}

pub struct CacheStateUnknown<'c, T, C, X, F, R> where X: FnOnce(&T) -> bool, F: FnOnce() -> T, C: FastCacheGet<'c, T, R> + FastCacheExpiration<T> {
    cache: C,
    expiration: X,
    fill: F,
    phantom: PhantomData<(&'c C, R)>
}

pub struct CacheStateKnown<T> {
    data: T,
}

pub trait MaybeGet<R> {
    fn get(&mut self) -> Option<&R>;

    fn take(self) -> Option<R>;
}

pub struct MaybeCacheAccesor<'c, T, C, X, F, R> where F: FnOnce() -> Option<T>, X: FnOnce(&T) -> bool, C: FastCacheMaybeGet<'c, T, R> + FastCacheExpiration<T> {
    state: MaybeCacheState<'c, T, C, X, F, R>,
    phantom: PhantomData<R>
}

impl<'c, T, C, X, F, R> MaybeGet<R> for MaybeCacheAccesor<'c, T, C, X, F, R> where F: FnOnce() -> Option<T>, X: FnOnce(&T) -> bool, C: FastCacheMaybeGet<'c, T, R> + FastCacheExpiration<T> {
    fn get(&mut self) -> Option<&R> {
        take_mut::take(&mut self.state, |v| v.into_known());

        match &self.state {
            MaybeCacheState::Unknown(_, _) => { unsafe { std::hint::unreachable_unchecked() } },
            MaybeCacheState::Known(s) => s.data.as_ref(),
        }
    }

    fn take(self) -> Option<R> {
        match self.state.into_known() {
            MaybeCacheState::Unknown(_, _) => { unsafe { std::hint::unreachable_unchecked() } },
            MaybeCacheState::Known(s) => s.data,
        }
    }
}

pub enum MaybeCacheState<'c, T, C, X, F, R> where F: FnOnce() -> Option<T>, X: FnOnce(&T) -> bool, C: FastCacheMaybeGet<'c, T, R> + FastCacheExpiration<T> {
    Unknown(MaybeCacheStateUnknown<'c, T, C, X, F, R>, PhantomData<R>),
    Known(MaybeCacheStateKnown<R>)
}

impl<'c, T, C, X, F, R> MaybeCacheState<'c, T, C, X, F, R> where F: FnOnce() -> Option<T>, X: FnOnce(&T) -> bool, C: FastCacheMaybeGet<'c, T, R> + FastCacheExpiration<T> {
    pub fn into_known(self) -> Self {
        match self {
            MaybeCacheState::Unknown(state, _) => {
                let ref_val = state.cache
                    .expire_with(state.expiration)
                    .maybe_get_or_insert_with(state.fill);
        
                let new_state = MaybeCacheStateKnown {
                    data: ref_val
                };

                MaybeCacheState::Known(new_state)            
            },
            v => v
        }
    }
}

pub struct MaybeCacheStateUnknown<'c, T, C, X, F, R> where X: FnOnce(&T) -> bool, F: FnOnce() -> Option<T>, C: FastCacheMaybeGet<'c, T, R> + FastCacheExpiration<T> {
    cache: C,
    expiration: X,
    fill: F,
    phantom: std::marker::PhantomData<(&'c C, R)>
}

pub struct MaybeCacheStateKnown<T> {
    data: Option<T>,
}