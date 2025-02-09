use std::borrow::Borrow;
use std::ffi::CString;
use std::fmt::{self, Debug};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::NonNull;

use crate::private::{get_api, ManuallyManagedClassPlaceholder, ReferenceCountedClassPlaceholder};
use crate::ref_kind::{ManuallyManaged, RefCounted, RefKind};
use crate::sys;
use crate::thread_access::{
    LocalThreadAccess, NonUniqueThreadAccess, Shared, ThreadAccess, ThreadLocal, Unique,
};

#[cfg(feature = "nativescript")]
use crate::nativescript::{Instance, NativeClass, RefInstance};

mod raw;

pub use self::raw::RawObject;

/// Trait for Godot API objects. This trait is sealed, and implemented for generated wrapper
/// types.
///
/// Bare `GodotObject` references, like `&Node`, can be used safely, but do not track thread
/// access states, which limits their usefulness to some extent. It's not, for example, possible
/// to pass a `&Node` into an API method because it might have came from a `Unique` reference.
/// As such, it's usually better to use `Ref` and `TRef`s whenever possible.
///
/// For convenience. it's possible to use bare references as `owner` arguments in exported
/// methods when using NativeScript, but the limitations above should be kept in mind. See
/// the `OwnerArg` for more information.
///
/// IF it's ever needed to obtain persistent references out of bare references, the `assume_`
/// methods can be used.
pub unsafe trait GodotObject: Sized + crate::private::godot_object::Sealed {
    /// The memory management kind of this type. This modifies the behavior of the
    /// [`Ref`](struct.Ref.html) smart pointer. See its type-level documentation for more
    /// information.
    type RefKind: RefKind;

    fn class_name() -> &'static str;

    /// Creates an explicitly null reference of `Self` as a method argument. This makes type
    /// inference easier for the compiler compared to `Option`.
    #[inline]
    fn null() -> Null<Self> {
        Null::null()
    }

    /// Creates a new instance of `Self` using a zero-argument constructor, as a `Unique`
    /// reference.
    #[inline]
    fn new() -> Ref<Self, Unique>
    where
        Self: Instanciable,
    {
        Ref::new()
    }

    /// Performs a dynamic reference downcast to target type.
    ///
    /// The `cast` method can only be used for downcasts. For statically casting to a
    /// supertype, use `upcast` instead.
    ///
    /// This method is only for conversion between engine types. To downcast to a `NativeScript`
    /// type from its base type, see `Ref::cast_instance` and `TRef::cast_instance`.
    #[inline]
    fn cast<T>(&self) -> Option<&T>
    where
        T: GodotObject + SubClass<Self>,
    {
        self.as_raw().cast().map(T::cast_ref)
    }

    /// Performs a static reference upcast to a supertype that is guaranteed to be valid.
    ///
    /// This is guaranteed to be a no-op at runtime.
    #[inline(always)]
    fn upcast<T>(&self) -> &T
    where
        T: GodotObject,
        Self: SubClass<T>,
    {
        unsafe { T::cast_ref(self.as_raw().cast_unchecked()) }
    }

    /// Creates a reference to `Self` given a `RawObject` reference. This is an internal
    /// interface,
    #[doc(hidden)]
    #[inline]
    fn cast_ref(raw: &RawObject<Self>) -> &Self {
        unsafe { &*(raw as *const _ as *const _) }
    }

    /// Casts `self` to `RawObject`. This is an internal interface.
    #[doc(hidden)]
    #[inline]
    fn as_raw(&self) -> &RawObject<Self> {
        unsafe { &*(self as *const _ as *const _) }
    }

    /// Casts `self` to a raw pointer. This is an internal interface.
    #[doc(hidden)]
    #[inline]
    fn as_ptr(&self) -> *mut sys::godot_object {
        self.as_raw().sys().as_ptr()
    }

    /// Creates a persistent reference to the same Godot object with shared thread access.
    ///
    /// # Safety
    ///
    /// There must not be any `Unique` or `ThreadLocal` references of the object when this
    /// is called. This causes undefined behavior otherwise.
    #[inline]
    unsafe fn assume_shared(&self) -> Ref<Self, Shared>
    where
        Self: Sized,
    {
        Ref::from_sys(self.as_raw().sys())
    }

    /// Creates a persistent reference to the same Godot object with thread-local thread access.
    ///
    /// # Safety
    ///
    /// There must not be any `Unique` or `Shared` references of the object when this
    /// is called. This causes undefined behavior otherwise.
    #[inline]
    unsafe fn assume_thread_local(&self) -> Ref<Self, ThreadLocal>
    where
        Self: Sized + GodotObject<RefKind = RefCounted>,
    {
        Ref::from_sys(self.as_raw().sys())
    }

    /// Creates a persistent reference to the same Godot object with unique access.
    ///
    /// # Safety
    ///
    /// **Use with care.** `Unique` is a very strong assumption that can easily be
    /// violated. Only use this when you are **absolutely** sure you have the only reference.
    ///
    /// There must be no other references of the object when this is called. This causes
    /// undefined behavior otherwise.
    #[inline]
    unsafe fn assume_unique(&self) -> Ref<Self, Unique>
    where
        Self: Sized,
    {
        Ref::from_sys(self.as_raw().sys())
    }

    /// Recovers a instance ID previously returned by `Object::get_instance_id` if the object is
    /// still alive. See also `TRef::try_from_instance_id`.
    ///
    /// # Safety
    ///
    /// During the entirety of `'a`, the thread from which `try_from_instance_id` is called must
    /// have exclusive access to the underlying object, if it is still alive.
    #[inline]
    unsafe fn try_from_instance_id<'a>(id: i64) -> Option<TRef<'a, Self, Shared>> {
        TRef::try_from_instance_id(id)
    }

    /// Recovers a instance ID previously returned by `Object::get_instance_id` if the object is
    /// still alive, and panics otherwise. This does **NOT** guarantee that the resulting
    /// reference is safe to use.
    ///
    /// # Panics
    ///
    /// Panics if the given id refers to a destroyed object. For a non-panicking version, see
    /// `try_from_instance_id`.
    ///
    /// # Safety
    ///
    /// During the entirety of `'a`, the thread from which `try_from_instance_id` is called must
    /// have exclusive access to the underlying object, if it is still alive.
    #[inline]
    unsafe fn from_instance_id<'a>(id: i64) -> TRef<'a, Self, Shared> {
        TRef::from_instance_id(id)
    }
}

/// Marker trait for API types that are subclasses of another type. This trait is implemented
/// by the bindings generator, and has no public interface. Users should not attempt to
/// implement this trait.
pub unsafe trait SubClass<A: GodotObject>: GodotObject {}
unsafe impl<T: GodotObject> SubClass<T> for T {}

/// GodotObjects that have a zero argument constructor.
pub trait Instanciable: GodotObject {
    fn construct() -> Ref<Self, Unique>;
}

/// Manually managed Godot classes implementing `queue_free`. This trait has no public
/// interface. See `Ref::queue_free`.
pub trait QueueFree: GodotObject {
    /// Deallocate the object in the near future.
    ///
    /// # Safety
    ///
    /// When this function is dequeued no references to this
    /// object must be held and dereferenced.
    #[doc(hidden)]
    unsafe fn godot_queue_free(sys: *mut sys::godot_object);
}

/// A polymorphic smart pointer for Godot objects whose behavior changes depending on the
/// memory management method of the underlying type and the thread access status.
///
/// # Manually-managed types
///
/// `Shared` references to manually-managed types, like `Ref<Node, Shared>`, act like raw
/// pointers. They are safe to alias, can be sent between threads, and can also be taken as
/// method arguments (converted from `Variant`). They can't be used directly. Instead, it's
/// required to obtain a safe view first. See the "Obtaining a safe view" section below for
/// more information.
///
/// `ThreadLocal` references to manually-managed types cannot normally be obtained, since
/// it does not add anything over `Shared` ones.
///
/// `Unique` references to manually-managed types, like `Ref<Node, Unique>`, can't be aliased
/// or sent between threads, but can be used safely. However, they *won't* be automatically
/// freed on drop, and are *leaked* if not passed to the engine or freed manually with `free`.
/// `Unique` references can be obtained through constructors safely, or `assume_unique` in
/// unsafe contexts.
///
/// # Reference-counted types
///
/// `Shared` references to reference-counted types, like `Ref<Reference, Shared>`, act like
/// `Arc` smart pointers. New references can be created with `Clone`, and they can be sent
/// between threads. The pointer is presumed to be always valid. As such, more operations
/// are available even when thread safety is not assumed. However, API methods still can't be
/// used directly, and users are required to obtain a safe view first. See the "Obtaining a
/// safe view" section below for more information.
///
/// `ThreadLocal` reference to reference-counted types, like `Ref<Reference, ThreadLocal>`, add
/// the ability to call API methods safely. Unlike `Unique` references, it's unsafe to convert
/// them to `Shared` because there might be other `ThreadLocal` references in existence.
///
/// # Obtaining a safe view
///
/// In a lot of cases, references obtained from the engine as return values or arguments aren't
/// safe to use, due to lack of pointer validity and thread safety guarantees in the API. As
/// such, it's usually required to use `unsafe` code to obtain safe views of the same object
/// before API methods can be called. The ways to cast between different reference types are as
/// follows:
///
/// | From | To | Method | Note |
/// | - | - | - | - |
/// | `Unique` | `&'a T` | `Deref` (API methods can be called directly) / `as_ref` | - |
/// | `ThreadLocal` | `&'a T` | `Deref` (API methods can be called directly) / `as_ref` | Only if `T` is a reference-counted type. |
/// | `Shared` | `&'a T` | `unsafe assume_safe::<'a>` | The underlying object must be valid, and exclusive to this thread during `'a`. |
/// | `Unique` | `ThreadLocal` | `into_thread_local` | - |
/// | `Unique` | `Shared` | `into_shared` | - |
/// | `Shared` | `ThreadLocal` | `unsafe assume_thread_local` | The reference must be local to the current thread. |
/// | `Shared` / `ThreadLocal` | `Unique` | `unsafe assume_unique` | The reference must be unique. |
/// | `ThreadLocal` | `Shared` | `unsafe assume_unique().into_shared()` | The reference must be unique. |
///
/// # Using as method arguments or return values
///
/// In order to enforce thread safety statically, the ability to be passed to the engine is only
/// given to some reference types. Specifically, they are:
///
/// - All *owned* `Ref<T, Unique>` references. The `Unique` access is lost if passed into a
///   method.
/// - Owned and borrowed `Shared` references, including temporary ones (`TRef`).
///
/// It's unsound to pass `ThreadLocal` references to the engine because there is no guarantee
/// that the reference will stay on the same thread.
///
/// # Conditional trait implementations
///
/// Many trait implementations for `Ref` are conditional, dependent on the type parameters.
/// When viewing rustdoc documentation, you may expand the documentation on their respective
/// `impl` blocks  for more detailed explanations of the trait bounds.
pub struct Ref<T: GodotObject, Access: ThreadAccess = Shared> {
    ptr: <T::RefKind as RefKindSpec>::PtrWrapper,
    _marker: PhantomData<(*const T, Access)>,
}

/// `Ref` is `Send` if the thread access is `Shared` or `Unique`.
unsafe impl<T: GodotObject, Access: ThreadAccess + Send> Send for Ref<T, Access> {}

/// `Ref` is `Sync` if the thread access is `Shared`.
unsafe impl<T: GodotObject, Access: ThreadAccess + Sync> Sync for Ref<T, Access> {}

impl<T: GodotObject, Access: ThreadAccess> private::Sealed for Ref<T, Access> {}

/// `Ref` is `Copy` if the underlying object is manually-managed, and the access is not
/// `Unique`.
impl<T, Access> Copy for Ref<T, Access>
where
    T: GodotObject<RefKind = ManuallyManaged>,
    Access: NonUniqueThreadAccess,
{
}

/// `Ref` is `Clone` if the access is not `Unique`.
impl<T, Access> Clone for Ref<T, Access>
where
    T: GodotObject,
    Access: NonUniqueThreadAccess,
{
    #[inline]
    fn clone(&self) -> Self {
        unsafe { Ref::from_sys(self.ptr.as_non_null()) }
    }
}

impl<T: GodotObject + Instanciable> Ref<T, Unique> {
    /// Creates a new instance of `T`.
    ///
    /// The lifetime of the returned object is *not* automatically managed if `T` is a manually-
    /// managed type.
    #[inline]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        T::construct()
    }
}

impl<T: GodotObject> Ref<T, Unique> {
    /// Creates a new instance of a sub-class of `T` by its class name. Returns `None` if the
    /// class does not exist, cannot be constructed, has a different `RefKind` from, or is not
    /// a sub-class of `T`.
    ///
    /// The lifetime of the returned object is *not* automatically managed if `T` is a manually-
    /// managed type. This means that if `Object` is used as the type parameter, any `Reference`
    /// objects created, if returned, will be leaked. As a result, such calls will return `None`.
    /// Casting between `Object` and `Reference` is possible on `TRef` and bare references.
    #[inline]
    pub fn by_class_name(class_name: &str) -> Option<Self> {
        unsafe {
            // Classes with NUL-bytes in their names can not exist
            let class_name = CString::new(class_name).ok()?;
            let ctor = (get_api().godot_get_class_constructor)(class_name.as_ptr())?;
            let ptr = NonNull::new(ctor() as *mut sys::godot_object)?;
            <T::RefKind as RefKindSpec>::impl_from_maybe_ref_counted(ptr)
        }
    }
}

/// Method for references that can be safely used.
impl<T: GodotObject, Access: ThreadAccess> Ref<T, Access>
where
    RefImplBound: SafeDeref<T::RefKind, Access>,
{
    /// Returns a safe temporary reference that tracks thread access.
    ///
    /// `Ref<T, Access>` can be safely dereferenced if either:
    ///
    /// - `T` is reference-counted and `Access` is not `Shared`,
    /// - or, `T` is manually-managed and `Access` is `Unique`.
    #[inline]
    pub fn as_ref(&self) -> TRef<'_, T, Access> {
        RefImplBound::impl_as_ref(self)
    }
}

/// `Ref<T, Access>` can be safely dereferenced if either:
///
/// - `T` is reference-counted and `Access` is not `Shared`,
/// - or, `T` is manually-managed and `Access` is `Unique`.
impl<T: GodotObject, Access: ThreadAccess> Deref for Ref<T, Access>
where
    RefImplBound: SafeDeref<T::RefKind, Access>,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        RefImplBound::impl_as_ref(self).obj
    }
}

/// `Ref<T, Access>` can be safely dereferenced if either:
///
/// - `T` is reference-counted and `Access` is not `Shared`,
/// - or, `T` is manually-managed and `Access` is `Unique`.
impl<T: GodotObject, Access: ThreadAccess> Borrow<T> for Ref<T, Access>
where
    RefImplBound: SafeDeref<T::RefKind, Access>,
{
    #[inline]
    fn borrow(&self) -> &T {
        RefImplBound::impl_as_ref(self).obj
    }
}

/// Methods for references that point to valid objects, but are not necessarily safe to use.
///
/// - All `Ref`s to reference-counted types always point to valid objects.
/// - `Ref` to manually-managed types are only guaranteed to be valid if `Unique`.
impl<T: GodotObject, Access: ThreadAccess> Ref<T, Access>
where
    RefImplBound: SafeAsRaw<T::RefKind, Access>,
{
    /// Cast to a `RawObject` reference safely. This is an internal interface.
    #[inline]
    #[doc(hidden)]
    pub fn as_raw(&self) -> &RawObject<T> {
        unsafe { self.as_raw_unchecked() }
    }

    /// Performs a dynamic reference cast to target type, keeping the reference count.
    /// Shorthand for `try_cast().ok()`.
    ///
    /// The `cast` method can only be used for downcasts. For statically casting to a
    /// supertype, use `upcast` instead.
    ///
    /// This is only possible between types with the same `RefKind`s, since otherwise the
    /// reference can get leaked. Casting between `Object` and `Reference` is possible on
    /// `TRef` and bare references.
    #[inline]
    pub fn cast<U>(self) -> Option<Ref<U, Access>>
    where
        U: GodotObject<RefKind = T::RefKind> + SubClass<T>,
    {
        self.try_cast().ok()
    }

    /// Performs a static reference upcast to a supertype, keeping the reference count.
    /// This is guaranteed to be valid.
    ///
    /// This is only possible between types with the same `RefKind`s, since otherwise the
    /// reference can get leaked. Casting between `Object` and `Reference` is possible on
    /// `TRef` and bare references.
    #[inline]
    pub fn upcast<U>(self) -> Ref<U, Access>
    where
        U: GodotObject<RefKind = T::RefKind>,
        T: SubClass<U>,
    {
        unsafe { self.cast_unchecked() }
    }

    /// Performs a dynamic reference cast to target type, keeping the reference count.
    ///
    /// This is only possible between types with the same `RefKind`s, since otherwise the
    /// reference can get leaked. Casting between `Object` and `Reference` is possible on
    /// `TRef` and bare references.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if the cast failed.
    #[inline]
    pub fn try_cast<U>(self) -> Result<Ref<U, Access>, Self>
    where
        U: GodotObject<RefKind = T::RefKind> + SubClass<T>,
    {
        if self.as_raw().is_class::<U>() {
            Ok(unsafe { self.cast_unchecked() })
        } else {
            Err(self)
        }
    }

    /// Performs an unchecked cast.
    unsafe fn cast_unchecked<U>(self) -> Ref<U, Access>
    where
        U: GodotObject<RefKind = T::RefKind>,
    {
        let ret = Ref::move_from_sys(self.ptr.as_non_null());
        std::mem::forget(self);
        ret
    }

    /// Performs a downcast to a `NativeClass` instance, keeping the reference count.
    /// Shorthand for `try_cast_instance().ok()`.
    ///
    /// The resulting `Instance` is not necessarily safe to use directly.
    #[inline]
    #[cfg(feature = "nativescript")]
    pub fn cast_instance<C>(self) -> Option<Instance<C, Access>>
    where
        C: NativeClass<Base = T>,
    {
        self.try_cast_instance().ok()
    }

    /// Performs a downcast to a `NativeClass` instance, keeping the reference count.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if the cast failed.
    #[inline]
    #[cfg(feature = "nativescript")]
    pub fn try_cast_instance<C>(self) -> Result<Instance<C, Access>, Self>
    where
        C: NativeClass<Base = T>,
    {
        Instance::try_from_base(self)
    }
}

/// Methods for references that can't be used directly, and have to be assumed safe `unsafe`ly.
impl<T: GodotObject> Ref<T, Shared> {
    /// Assume that `self` is safe to use, returning a reference that can be used to call API
    /// methods.
    ///
    /// This is guaranteed to be a no-op at runtime if `debug_assertions` is disabled. Runtime
    /// sanity checks may be added in debug builds to help catch bugs.
    ///
    /// # Safety
    ///
    /// Suppose that the lifetime of the returned reference is `'a`. It's safe to call
    /// `assume_safe` only if:
    ///
    /// 1. During the entirety of `'a`, the underlying object will always be valid.
    ///
    ///     *This is always true for reference-counted types.* For them, the `'a` lifetime will
    ///     be constrained to the lifetime of `&self`.
    ///
    ///     This means that any methods called on the resulting reference will not free it,
    ///     unless it's the last operation within the lifetime.
    ///
    ///     If any script methods are called, the code ran as a consequence will also not free
    ///     it. This can happen via virtual method calls on other objects, or signals connected
    ///     in a non-deferred way.
    ///
    /// 2. During the entirety of 'a, the thread from which `assume_safe` is called has
    ///    exclusive access to the underlying object.
    ///
    ///     This is because all Godot objects have "interior mutability" in Rust parlance,
    ///     and can't be shared across threads. The best way to guarantee this is to follow
    ///     the official [thread-safety guidelines][thread-safety] across the codebase.
    ///
    /// Failure to satisfy either of the conditions will lead to undefined behavior.
    ///
    /// [thread-safety]: https://docs.godotengine.org/en/stable/tutorials/threads/thread_safe_apis.html
    #[inline(always)]
    pub unsafe fn assume_safe<'a, 'r>(&'r self) -> TRef<'a, T, Shared>
    where
        AssumeSafeLifetime<'a, 'r>: LifetimeConstraint<T::RefKind>,
    {
        T::RefKind::impl_assume_safe(self)
    }

    /// Assume that `self` is the unique reference to the underlying object.
    ///
    /// This is guaranteed to be a no-op at runtime if `debug_assertions` is disabled. Runtime
    /// sanity checks may be added in debug builds to help catch bugs.
    ///
    /// # Safety
    ///
    /// Calling `assume_unique` when `self` isn't the unique reference is instant undefined
    /// behavior. This is a much stronger assumption than `assume_safe` and should be used with
    /// care.
    #[inline(always)]
    pub unsafe fn assume_unique(self) -> Ref<T, Unique> {
        T::RefKind::impl_assume_unique(self)
    }
}

/// Extra methods with explicit sanity checks for manually-managed unsafe references.
impl<T: GodotObject<RefKind = ManuallyManaged>> Ref<T, Shared> {
    /// Returns `true` if the pointer currently points to a valid object of the correct type.
    /// **This does NOT guarantee that it's safe to use this pointer.**
    ///
    /// # Safety
    ///
    /// This thread must have exclusive access to the object during the call.
    #[inline]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub unsafe fn is_instance_sane(&self) -> bool {
        let api = get_api();
        if !(api.godot_is_instance_valid)(self.as_ptr()) {
            return false;
        }

        self.as_raw_unchecked().is_class::<T>()
    }

    /// Assume that `self` is safe to use, if a sanity check using `is_instance_sane` passed.
    ///
    /// # Safety
    ///
    /// The same safety constraints as `assume_safe` applies. **The sanity check does NOT
    /// guarantee that the operation is safe.**
    #[inline]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub unsafe fn assume_safe_if_sane<'a>(&self) -> Option<TRef<'a, T, Shared>> {
        if self.is_instance_sane() {
            Some(self.assume_safe_unchecked())
        } else {
            None
        }
    }

    /// Assume that `self` is the unique reference to the underlying object, if a sanity check
    /// using `is_instance_sane` passed.
    ///
    /// # Safety
    ///
    /// Calling `assume_unique_if_sane` when `self` isn't the unique reference is instant
    /// undefined behavior. This is a much stronger assumption than `assume_safe` and should be
    /// used with care.
    #[inline]
    pub unsafe fn assume_unique_if_sane(self) -> Option<Ref<T, Unique>> {
        if self.is_instance_sane() {
            Some(self.cast_access())
        } else {
            None
        }
    }
}

/// Methods for conversion from `Shared` to `ThreadLocal` access. This is only available for
/// reference-counted types.
impl<T: GodotObject<RefKind = RefCounted>> Ref<T, Shared> {
    /// Assume that all references to the underlying object is local to the current thread.
    ///
    /// This is guaranteed to be a no-op at runtime.
    ///
    /// # Safety
    ///
    /// Calling `assume_thread_local` when there are references on other threads is instant
    /// undefined behavior. This is a much stronger assumption than `assume_safe` and should
    /// be used with care.
    #[inline(always)]
    pub unsafe fn assume_thread_local(self) -> Ref<T, ThreadLocal> {
        self.cast_access()
    }
}

/// Methods for conversion from `Unique` to `ThreadLocal` access. This is only available for
/// reference-counted types.
impl<T: GodotObject<RefKind = RefCounted>> Ref<T, Unique> {
    /// Convert to a thread-local reference.
    ///
    /// This is guaranteed to be a no-op at runtime.
    #[inline(always)]
    pub fn into_thread_local(self) -> Ref<T, ThreadLocal> {
        unsafe { self.cast_access() }
    }
}

/// Methods for conversion from `Unique` to `Shared` access.
impl<T: GodotObject> Ref<T, Unique> {
    /// Convert to a shared reference.
    ///
    /// This is guaranteed to be a no-op at runtime.
    #[inline(always)]
    pub fn into_shared(self) -> Ref<T, Shared> {
        unsafe { self.cast_access() }
    }
}

/// Methods for freeing `Unique` references to manually-managed objects.
impl<T: GodotObject<RefKind = ManuallyManaged>> Ref<T, Unique> {
    /// Manually frees the object.
    ///
    /// Manually-managed objects are not free-on-drop *even when the access is unique*, because
    /// it's impossible to know whether methods take "ownership" of them or not. It's up to the
    /// user to decide when they should be freed.
    ///
    /// This is only available for `Unique` references. If you have a `Ref` with another access,
    /// and you are sure that it is unique, use `assume_unique` to convert it to a `Unique` one.
    #[inline]
    pub fn free(self) {
        unsafe {
            self.as_raw().free();
        }
    }
}

/// Methods for freeing `Unique` references to manually-managed objects.
impl<T: GodotObject<RefKind = ManuallyManaged> + QueueFree> Ref<T, Unique> {
    /// Queues the object for deallocation in the near future. This is preferable for `Node`s
    /// compared to `Ref::free`.
    ///
    /// This is only available for `Unique` references. If you have a `Ref` with another access,
    /// and you are sure that it is unique, use `assume_unique` to convert it to a `Unique` one.
    #[inline]
    pub fn queue_free(self) {
        unsafe { T::godot_queue_free(self.as_ptr()) }
    }
}

/// Reference equality.
impl<T: GodotObject, Access: ThreadAccess> Eq for Ref<T, Access> {}
/// Reference equality.
impl<T, Access, RhsAccess> PartialEq<Ref<T, RhsAccess>> for Ref<T, Access>
where
    T: GodotObject,
    Access: ThreadAccess,
    RhsAccess: ThreadAccess,
{
    #[inline]
    fn eq(&self, other: &Ref<T, RhsAccess>) -> bool {
        self.ptr.as_non_null() == other.ptr.as_non_null()
    }
}

/// Ordering of the raw pointer value.
impl<T: GodotObject, Access: ThreadAccess> Ord for Ref<T, Access> {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.ptr.as_non_null().cmp(&other.ptr.as_non_null())
    }
}

/// Ordering of the raw pointer value.
impl<T: GodotObject, Access: ThreadAccess> PartialOrd for Ref<T, Access> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.ptr.as_non_null().partial_cmp(&other.ptr.as_non_null())
    }
}

/// Hashes the raw pointer.
impl<T: GodotObject, Access: ThreadAccess> Hash for Ref<T, Access> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.ptr.as_ptr() as usize)
    }
}

impl<T: GodotObject, Access: ThreadAccess> Debug for Ref<T, Access> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:p})", T::class_name(), self.ptr.as_ptr())
    }
}

impl<T: GodotObject, Access: ThreadAccess> Ref<T, Access> {
    /// Convert to a nullable raw pointer.
    #[doc(hidden)]
    #[inline]
    pub fn sys(&self) -> *mut sys::godot_object {
        self.ptr.as_ptr()
    }

    /// Convert to a nullable raw pointer.
    #[doc(hidden)]
    #[inline]
    pub fn as_ptr(&self) -> *mut sys::godot_object {
        self.ptr.as_ptr()
    }

    /// Convert to a `RawObject` reference.
    ///
    /// # Safety
    ///
    /// `self` must point to a valid object of the correct type.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn as_raw_unchecked<'a>(&self) -> &'a RawObject<T> {
        RawObject::from_sys_ref_unchecked(self.ptr.as_non_null())
    }

    /// Convert from a pointer returned from a ptrcall. For reference-counted types, this takes
    /// the ownership of the returned reference, in Rust parlance. For non-reference-counted
    /// types, its behavior should be exactly the same as `from_sys`. This is needed for
    /// reference-counted types to be properly freed, since any references returned from
    /// ptrcalls are leaked in the process of being cast into a pointer.
    ///
    /// # Safety
    ///
    /// `obj` must point to a valid object of the correct type.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn move_from_sys(obj: NonNull<sys::godot_object>) -> Self {
        Ref {
            ptr: <T::RefKind as RefKindSpec>::PtrWrapper::new(obj),
            _marker: PhantomData,
        }
    }

    /// Convert from a raw pointer, incrementing the reference counter if reference-counted.
    ///
    /// # Safety
    ///
    /// `obj` must point to a valid object of the correct type.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn from_sys(obj: NonNull<sys::godot_object>) -> Self {
        let ret = Self::move_from_sys(obj);
        <T::RefKind as RefKindSpec>::maybe_add_ref(ret.as_raw_unchecked());
        ret
    }

    /// Convert from a pointer returned from a constructor of a reference-counted type. For
    /// non-reference-counted types, its behavior should be exactly the same as `from_sys`.
    ///
    /// # Safety
    ///
    /// `obj` must point to a valid object of the correct type, and must be the only reference.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn init_from_sys(obj: NonNull<sys::godot_object>) -> Self {
        let ret = Self::move_from_sys(obj);
        <T::RefKind as RefKindSpec>::maybe_init_ref(ret.as_raw_unchecked());
        ret
    }

    /// Casts the access type of `self` to `TargetAccess`, moving the reference.
    ///
    /// # Safety
    ///
    /// The cast must be valid.
    unsafe fn cast_access<TargetAccess: ThreadAccess>(self) -> Ref<T, TargetAccess> {
        let ret = Ref::move_from_sys(self.ptr.as_non_null());
        std::mem::forget(self);
        ret
    }

    /// Assume that the reference is safe in an `unsafe` context even if it can be used safely.
    /// For internal use in macros.
    ///
    /// This is guaranteed to be a no-op at runtime.
    ///
    /// # Safety
    ///
    /// The same safety constraints as `assume_safe` applies.
    #[doc(hidden)]
    #[inline(always)]
    pub unsafe fn assume_safe_unchecked<'a>(&self) -> TRef<'a, T, Access> {
        TRef::new(T::cast_ref(self.as_raw_unchecked()))
    }
}

/// A temporary safe pointer to Godot objects that tracks thread access status. `TRef` can be
/// coerced into bare references with `Deref`.
///
/// See the type-level documentation on `Ref` for detailed documentation on the reference
/// system of `godot-rust`.
///
/// # Using as method arguments or return values
///
/// `TRef<T, Shared>` can be passed into methods.
///
/// # Using as `owner` arguments in NativeScript methods
///
/// It's possible to use `TRef` as the `owner` argument in NativeScript methods. This can make
/// passing `owner` to methods easier.
pub struct TRef<'a, T: GodotObject, Access: ThreadAccess = Shared> {
    obj: &'a T,
    _marker: PhantomData<Access>,
}

impl<'a, T: GodotObject, Access: ThreadAccess> Copy for TRef<'a, T, Access> {}
impl<'a, T: GodotObject, Access: ThreadAccess> Clone for TRef<'a, T, Access> {
    #[inline]
    fn clone(&self) -> Self {
        TRef::new(self.obj)
    }
}

impl<'a, T: GodotObject, Access: ThreadAccess> Debug for TRef<'a, T, Access> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:p})", T::class_name(), self.obj)
    }
}

impl<'a, T: GodotObject, Access: ThreadAccess> Deref for TRef<'a, T, Access> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.obj
    }
}

impl<'a, T: GodotObject, Access: ThreadAccess> AsRef<T> for TRef<'a, T, Access> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.obj
    }
}

impl<'a, T: GodotObject, Access: ThreadAccess> Borrow<T> for TRef<'a, T, Access> {
    #[inline]
    fn borrow(&self) -> &T {
        self.obj
    }
}

impl<'a, T: GodotObject, Access: ThreadAccess> TRef<'a, T, Access> {
    pub(crate) fn new(obj: &'a T) -> Self {
        TRef {
            obj,
            _marker: PhantomData,
        }
    }

    /// Returns the underlying reference without thread access.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn as_ref(self) -> &'a T {
        self.obj
    }

    /// Performs a dynamic reference cast to target type, keeping the thread access info.
    #[inline]
    pub fn cast<U>(self) -> Option<TRef<'a, U, Access>>
    where
        U: GodotObject + SubClass<T>,
    {
        self.obj.cast().map(TRef::new)
    }

    /// Performs a static reference upcast to a supertype that is guaranteed to be valid,
    /// keeping the thread access info.
    ///
    /// This is guaranteed to be a no-op at runtime.
    #[inline(always)]
    pub fn upcast<U>(&self) -> TRef<'a, U, Access>
    where
        U: GodotObject,
        T: SubClass<U>,
    {
        TRef::new(self.obj.upcast())
    }

    /// Convenience method to downcast to `RefInstance` where `self` is the base object.
    #[inline]
    #[cfg(feature = "nativescript")]
    pub fn cast_instance<C>(self) -> Option<RefInstance<'a, C, Access>>
    where
        C: NativeClass<Base = T>,
    {
        RefInstance::try_from_base(self)
    }
}

impl<'a, Kind, T, Access> TRef<'a, T, Access>
where
    Kind: RefKind,
    T: GodotObject<RefKind = Kind>,
    Access: NonUniqueThreadAccess,
{
    /// Persists this reference into a persistent `Ref` with the same thread access.
    ///
    /// This is only available for non-`Unique` accesses.
    #[inline]
    pub fn claim(self) -> Ref<T, Access> {
        unsafe { Ref::from_sys(self.obj.as_raw().sys()) }
    }
}

impl<'a, T: GodotObject> TRef<'a, T, Shared> {
    /// Recovers a instance ID previously returned by `Object::get_instance_id` if the object is
    /// still alive.
    ///
    /// # Safety
    ///
    /// During the entirety of `'a`, the thread from which `try_from_instance_id` is called must
    /// have exclusive access to the underlying object, if it is still alive.
    #[inline]
    pub unsafe fn try_from_instance_id(id: i64) -> Option<Self> {
        let api = get_api();
        let ptr = NonNull::new((api.godot_instance_from_id)(id as sys::godot_int))?;
        let raw = RawObject::try_from_sys_ref(ptr)?;
        Some(TRef::new(T::cast_ref(raw)))
    }

    /// Recovers a instance ID previously returned by `Object::get_instance_id` if the object is
    /// still alive, and panics otherwise. This does **NOT** guarantee that the resulting
    /// reference is safe to use.
    ///
    /// # Panics
    ///
    /// Panics if the given id refers to a destroyed object. For a non-panicking version, see
    /// `try_from_instance_id`.
    ///
    /// # Safety
    ///
    /// During the entirety of `'a`, the thread from which `try_from_instance_id` is called must
    /// have exclusive access to the underlying object, if it is still alive.
    #[inline]
    pub unsafe fn from_instance_id(id: i64) -> Self {
        Self::try_from_instance_id(id).expect("instance should be alive")
    }
}

/// Trait for safe conversion from Godot object references into API method arguments. This is
/// a sealed trait with no public interface.
///
/// In order to enforce thread safety statically, the ability to be passed to the engine is only
/// given to some reference types. Specifically, they are:
///
/// - All *owned* `Ref<T, Unique>` references. The `Unique` access is lost if passed into a
///   method.
/// - Owned and borrowed `Shared` references, including temporary ones (`TRef`).
///
/// It's unsound to pass `ThreadLocal` references to the engine because there is no guarantee
/// that the reference will stay on the same thread.
///
/// To explicitly pass a null reference to the engine, use `Null::null` or `GodotObject::null`.
pub trait AsArg<T>: private::Sealed {
    #[doc(hidden)]
    fn as_arg_ptr(&self) -> *mut sys::godot_object;

    #[doc(hidden)]
    #[inline]
    unsafe fn to_arg_variant(&self) -> crate::core_types::Variant {
        crate::core_types::Variant::from_object_ptr(self.as_arg_ptr())
    }
}

/// Trait for safe conversion from Godot object references into Variant. This is
/// a sealed trait with no public interface.
///
/// Used for `Variant` methods and implementations as a trait bound to improve type inference.
pub trait AsVariant: AsArg<<Self as AsVariant>::Target> {
    type Target;
}

/// Represents an explicit null reference in method arguments. This works around type inference
/// issues with `Option`. You may create `Null`s with `Null::null` or `GodotObject::null`.
pub struct Null<T>(PhantomData<T>);

impl<T: GodotObject> Null<T> {
    /// Creates an explicit null reference that can be used as a method argument.
    // TODO consider something more idiomatic, like module::null::<T>(), similar to std::ptr::null()
    #[inline]
    #[allow(clippy::self_named_constructors)]
    pub fn null() -> Self {
        Null(PhantomData)
    }
}

impl<'a, T> private::Sealed for Null<T> {}
impl<'a, T: GodotObject> AsArg<T> for Null<T> {
    #[inline]
    fn as_arg_ptr(&self) -> *mut sys::godot_object {
        std::ptr::null_mut()
    }
}
impl<'a, T: GodotObject> AsVariant for Null<T> {
    type Target = T;
}

impl<'a, T: GodotObject> private::Sealed for TRef<'a, T, Shared> {}
impl<'a, T, U> AsArg<U> for TRef<'a, T, Shared>
where
    T: GodotObject + SubClass<U>,
    U: GodotObject,
{
    #[inline]
    fn as_arg_ptr(&self) -> *mut sys::godot_object {
        self.as_ptr()
    }
}
impl<'a, T: GodotObject> AsVariant for TRef<'a, T, Shared> {
    type Target = T;
}

impl<T, U> AsArg<U> for Ref<T, Shared>
where
    T: GodotObject + SubClass<U>,
    U: GodotObject,
{
    #[inline]
    fn as_arg_ptr(&self) -> *mut sys::godot_object {
        self.as_ptr()
    }
}
impl<T: GodotObject> AsVariant for Ref<T, Shared> {
    type Target = T;
}

impl<T, U> AsArg<U> for Ref<T, Unique>
where
    T: GodotObject + SubClass<U>,
    U: GodotObject,
{
    #[inline]
    fn as_arg_ptr(&self) -> *mut sys::godot_object {
        self.as_ptr()
    }
}
impl<T: GodotObject> AsVariant for Ref<T, Unique> {
    type Target = T;
}

impl<'a, T: GodotObject> private::Sealed for &'a Ref<T, Shared> {}
impl<'a, T, U> AsArg<U> for &'a Ref<T, Shared>
where
    T: GodotObject + SubClass<U>,
    U: GodotObject,
{
    #[inline]
    fn as_arg_ptr(&self) -> *mut sys::godot_object {
        self.as_ptr()
    }
}
impl<'a, T: GodotObject> AsVariant for &'a Ref<T, Shared> {
    type Target = T;
}

/// Trait for combinations of `RefKind` and `ThreadAccess` that can be dereferenced safely.
/// This is an internal interface.
pub unsafe trait SafeDeref<Kind: RefKind, Access: ThreadAccess> {
    /// Returns a safe reference to the underlying object.
    #[doc(hidden)]
    fn impl_as_ref<T: GodotObject<RefKind = Kind>>(this: &Ref<T, Access>) -> TRef<'_, T, Access>;
}

/// Trait for persistent `Ref`s that point to valid objects. This is an internal interface.
pub unsafe trait SafeAsRaw<Kind: RefKind, Access: ThreadAccess> {
    /// Returns a raw reference to the underlying object.
    #[doc(hidden)]
    fn impl_as_raw<T: GodotObject<RefKind = Kind>>(this: &Ref<T, Access>) -> &RawObject<T>;
}

/// Struct to be used for various `Ref` trait bounds.
pub struct RefImplBound {
    _private: (),
}

unsafe impl SafeDeref<ManuallyManaged, Unique> for RefImplBound {
    #[inline]
    fn impl_as_ref<T: GodotObject<RefKind = ManuallyManaged>>(
        this: &Ref<T, Unique>,
    ) -> TRef<'_, T, Unique> {
        unsafe { this.assume_safe_unchecked() }
    }
}

unsafe impl<Access: LocalThreadAccess> SafeDeref<RefCounted, Access> for RefImplBound {
    #[inline]
    fn impl_as_ref<T: GodotObject<RefKind = RefCounted>>(
        this: &Ref<T, Access>,
    ) -> TRef<'_, T, Access> {
        unsafe { this.assume_safe_unchecked() }
    }
}

unsafe impl SafeAsRaw<ManuallyManaged, Unique> for RefImplBound {
    #[inline]
    fn impl_as_raw<T: GodotObject<RefKind = ManuallyManaged>>(
        this: &Ref<T, Unique>,
    ) -> &RawObject<T> {
        unsafe { this.as_raw_unchecked() }
    }
}

unsafe impl<Access: ThreadAccess> SafeAsRaw<RefCounted, Access> for RefImplBound {
    #[inline]
    fn impl_as_raw<T: GodotObject<RefKind = RefCounted>>(this: &Ref<T, Access>) -> &RawObject<T> {
        unsafe { this.as_raw_unchecked() }
    }
}

/// Specialization trait depending on `RefKind`. This is an internal interface.
pub trait RefKindSpec: Sized {
    /// Pointer wrapper that may be `Drop` or not.
    #[doc(hidden)]
    type PtrWrapper: PtrWrapper;

    #[doc(hidden)]
    unsafe fn impl_from_maybe_ref_counted<T: GodotObject<RefKind = Self>>(
        ptr: NonNull<sys::godot_object>,
    ) -> Option<Ref<T, Unique>>
    where
        Self: RefKind;

    #[doc(hidden)]
    unsafe fn impl_assume_safe<'a, T: GodotObject<RefKind = Self>>(
        this: &Ref<T, Shared>,
    ) -> TRef<'a, T, Shared>
    where
        Self: RefKind;

    #[doc(hidden)]
    unsafe fn impl_assume_unique<T: GodotObject<RefKind = Self>>(
        this: Ref<T, Shared>,
    ) -> Ref<T, Unique>
    where
        Self: RefKind;

    #[doc(hidden)]
    unsafe fn maybe_add_ref<T: GodotObject<RefKind = Self>>(raw: &RawObject<T>)
    where
        Self: RefKind;

    #[doc(hidden)]
    unsafe fn maybe_init_ref<T: GodotObject<RefKind = Self>>(raw: &RawObject<T>)
    where
        Self: RefKind;
}

impl RefKindSpec for ManuallyManaged {
    type PtrWrapper = Forget;

    #[inline(always)]
    unsafe fn impl_from_maybe_ref_counted<T: GodotObject<RefKind = Self>>(
        ptr: NonNull<sys::godot_object>,
    ) -> Option<Ref<T, Unique>> {
        if RawObject::<ReferenceCountedClassPlaceholder>::try_from_sys_ref(ptr).is_some() {
            drop(Ref::<ReferenceCountedClassPlaceholder, Unique>::init_from_sys(ptr));
            None
        } else {
            let obj = Ref::<ManuallyManagedClassPlaceholder, Unique>::init_from_sys(ptr);

            if obj.as_raw().is_class::<T>() {
                Some(obj.cast_unchecked())
            } else {
                obj.free();
                None
            }
        }
    }

    #[inline(always)]
    unsafe fn impl_assume_safe<'a, T: GodotObject<RefKind = Self>>(
        this: &Ref<T, Shared>,
    ) -> TRef<'a, T, Shared> {
        debug_assert!(
            this.is_instance_sane(),
            "assume_safe called on an invalid pointer"
        );
        this.assume_safe_unchecked()
    }

    #[inline(always)]
    unsafe fn impl_assume_unique<T: GodotObject<RefKind = Self>>(
        this: Ref<T, Shared>,
    ) -> Ref<T, Unique> {
        debug_assert!(
            this.is_instance_sane(),
            "assume_unique called on an invalid pointer"
        );
        this.cast_access()
    }

    #[inline]
    unsafe fn maybe_add_ref<T: GodotObject<RefKind = Self>>(_raw: &RawObject<T>) {}
    #[inline]
    unsafe fn maybe_init_ref<T: GodotObject<RefKind = Self>>(_raw: &RawObject<T>) {}
}

impl RefKindSpec for RefCounted {
    type PtrWrapper = UnRef;

    #[inline(always)]
    unsafe fn impl_from_maybe_ref_counted<T: GodotObject<RefKind = Self>>(
        ptr: NonNull<sys::godot_object>,
    ) -> Option<Ref<T, Unique>> {
        if RawObject::<ReferenceCountedClassPlaceholder>::try_from_sys_ref(ptr).is_some() {
            let obj = Ref::<ReferenceCountedClassPlaceholder, Unique>::init_from_sys(ptr);

            if obj.as_raw().is_class::<T>() {
                Some(obj.cast_unchecked())
            } else {
                None
            }
        } else {
            RawObject::<ManuallyManagedClassPlaceholder>::from_sys_ref_unchecked(ptr).free();
            None
        }
    }

    #[inline(always)]
    unsafe fn impl_assume_safe<'a, T: GodotObject<RefKind = Self>>(
        this: &Ref<T, Shared>,
    ) -> TRef<'a, T, Shared> {
        this.assume_safe_unchecked()
    }

    #[inline(always)]
    unsafe fn impl_assume_unique<T: GodotObject<RefKind = Self>>(
        this: Ref<T, Shared>,
    ) -> Ref<T, Unique> {
        this.cast_access()
    }

    #[inline]
    unsafe fn maybe_add_ref<T: GodotObject<RefKind = Self>>(raw: &RawObject<T>) {
        raw.add_ref();
    }

    #[inline]
    unsafe fn maybe_init_ref<T: GodotObject<RefKind = Self>>(raw: &RawObject<T>) {
        raw.init_ref_count();
    }
}

/// Specialization trait for `Drop` behavior.
pub trait PtrWrapper {
    fn new(ptr: NonNull<sys::godot_object>) -> Self;
    fn as_non_null(&self) -> NonNull<sys::godot_object>;

    #[inline]
    fn as_ptr(&self) -> *mut sys::godot_object {
        self.as_non_null().as_ptr()
    }
}

#[derive(Copy, Clone)]
pub struct Forget(NonNull<sys::godot_object>);
impl PtrWrapper for Forget {
    #[inline]
    fn new(ptr: NonNull<sys::godot_object>) -> Self {
        Forget(ptr)
    }

    #[inline]
    fn as_non_null(&self) -> NonNull<sys::godot_object> {
        self.0
    }
}

pub struct UnRef(NonNull<sys::godot_object>);
impl PtrWrapper for UnRef {
    #[inline]
    fn new(ptr: NonNull<sys::godot_object>) -> Self {
        UnRef(ptr)
    }

    #[inline]
    fn as_non_null(&self) -> NonNull<sys::godot_object> {
        self.0
    }
}
impl Drop for UnRef {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            let raw = RawObject::<ReferenceCountedClassPlaceholder>::from_sys_ref_unchecked(self.0);
            raw.unref_and_free_if_last();
        }
    }
}

/// Trait for constraining `assume_safe` lifetimes to the one of `&self` when `T` is
/// reference-counted. This is an internal interface.
pub trait LifetimeConstraint<Kind: RefKind> {}

/// Type used to check lifetime constraint depending on `RefKind`. Internal interface.
#[doc(hidden)]
pub struct AssumeSafeLifetime<'a, 'r> {
    _marker: PhantomData<(&'a (), &'r ())>,
}

impl<'a, 'r> LifetimeConstraint<ManuallyManaged> for AssumeSafeLifetime<'a, 'r> {}
impl<'a, 'r: 'a> LifetimeConstraint<RefCounted> for AssumeSafeLifetime<'a, 'r> {}

mod private {
    pub trait Sealed {}
}
