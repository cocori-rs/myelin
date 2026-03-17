use std::any::{Any, TypeId};
use std::collections::HashMap;

/// A type-safe context map for sharing data across middleware layers.
///
/// `GrpcContext` is keyed by `TypeId`, so each concrete type can appear at most
/// once. All stored values must be `Send + Sync + 'static` to cross tokio task
/// boundaries safely.
#[derive(Default)]
pub struct GrpcContext {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl GrpcContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a value. Overwrites if the same type already exists.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Get a shared reference. Returns `None` if not present.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref())
    }

    /// Get a mutable reference.
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_mut())
    }

    /// Check presence without borrowing the value.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Remove and take ownership.
    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast().ok())
            .map(|boxed| *boxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut ctx = GrpcContext::new();
        ctx.insert(42u32);
        assert_eq!(ctx.get::<u32>(), Some(&42));
    }

    #[test]
    fn get_returns_none_for_missing_type() {
        let ctx = GrpcContext::new();
        assert_eq!(ctx.get::<String>(), None);
    }

    #[test]
    fn insert_overwrites_same_type() {
        let mut ctx = GrpcContext::new();
        ctx.insert(1u64);
        ctx.insert(2u64);
        assert_eq!(ctx.get::<u64>(), Some(&2));
    }

    #[test]
    fn distinct_types_coexist() {
        let mut ctx = GrpcContext::new();
        ctx.insert(10u32);
        ctx.insert("hello".to_string());
        assert_eq!(ctx.get::<u32>(), Some(&10));
        assert_eq!(ctx.get::<String>(), Some(&"hello".to_string()));
    }

    #[test]
    fn get_mut_modifies_in_place() {
        let mut ctx = GrpcContext::new();
        ctx.insert(vec![1, 2, 3]);
        ctx.get_mut::<Vec<i32>>().unwrap().push(4);
        assert_eq!(ctx.get::<Vec<i32>>(), Some(&vec![1, 2, 3, 4]));
    }

    #[test]
    fn contains_reflects_presence() {
        let mut ctx = GrpcContext::new();
        assert!(!ctx.contains::<u8>());
        ctx.insert(0u8);
        assert!(ctx.contains::<u8>());
    }

    #[test]
    fn remove_returns_value_and_clears() {
        let mut ctx = GrpcContext::new();
        ctx.insert(99i32);
        assert_eq!(ctx.remove::<i32>(), Some(99));
        assert!(!ctx.contains::<i32>());
    }

    #[test]
    fn remove_returns_none_when_absent() {
        let mut ctx = GrpcContext::new();
        assert_eq!(ctx.remove::<f64>(), None);
    }

    #[derive(Debug, Clone, PartialEq)]
    struct AuthClaims {
        user_id: u64,
        role: String,
    }

    #[test]
    fn works_with_custom_structs() {
        let mut ctx = GrpcContext::new();
        let claims = AuthClaims {
            user_id: 42,
            role: "admin".into(),
        };
        ctx.insert(claims.clone());
        assert_eq!(ctx.get::<AuthClaims>(), Some(&claims));
    }
}
