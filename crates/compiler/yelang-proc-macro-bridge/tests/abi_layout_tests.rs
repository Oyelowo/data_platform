//! Stable C ABI layout tests.
//!
//! These tests verify the memory layout of the `#[repr(C)]` structures that cross
//! the compiler / proc-macro-server / dylib boundary. They must be updated
//! deliberately and only when `CURRENT_ABI_VERSION` is bumped.

use std::mem::{align_of, offset_of, size_of};

use yelang_proc_macro_bridge::abi::registrar::YelangProcMacroKind;
use yelang_proc_macro_bridge::abi::{
    YelangMacroDescriptor, YelangMacroInvoke, YelangProcMacroExports,
};

/// On 64-bit targets we assert exact offsets and sizes so that an accidental
/// field reorder or padding change fails CI. On other targets we only assert
/// monotonic offsets and alignment, because the C ABI is still valid but the
/// concrete numbers differ.
const IS_64BIT: bool = size_of::<usize>() == 8;

#[test]
fn exports_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<YelangProcMacroExports>();
}

#[test]
fn descriptor_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<YelangMacroDescriptor>();
}

#[test]
fn exports_layout_64bit() {
    assert_eq!(align_of::<YelangProcMacroExports>(), 8);

    if IS_64BIT {
        assert_eq!(size_of::<YelangProcMacroExports>(), 40);
        assert_eq!(offset_of!(YelangProcMacroExports, abi_version), 0);
        assert_eq!(offset_of!(YelangProcMacroExports, macro_count), 8);
        assert_eq!(offset_of!(YelangProcMacroExports, macros), 16);
        assert_eq!(offset_of!(YelangProcMacroExports, alloc), 24);
        assert_eq!(offset_of!(YelangProcMacroExports, free), 32);
    } else {
        // Offsets must be monotonic and aligned.
        assert!(
            offset_of!(YelangProcMacroExports, abi_version)
                < offset_of!(YelangProcMacroExports, macro_count)
        );
        assert!(
            offset_of!(YelangProcMacroExports, macro_count)
                < offset_of!(YelangProcMacroExports, macros)
        );
        assert!(
            offset_of!(YelangProcMacroExports, macros) < offset_of!(YelangProcMacroExports, alloc)
        );
        assert!(
            offset_of!(YelangProcMacroExports, alloc) < offset_of!(YelangProcMacroExports, free)
        );
    }
}

#[test]
fn descriptor_layout_64bit() {
    assert_eq!(align_of::<YelangMacroDescriptor>(), 8);

    if IS_64BIT {
        assert_eq!(size_of::<YelangMacroDescriptor>(), 48);
        assert_eq!(offset_of!(YelangMacroDescriptor, name), 0);
        assert_eq!(offset_of!(YelangMacroDescriptor, name_len), 8);
        assert_eq!(offset_of!(YelangMacroDescriptor, kind), 16);
        assert_eq!(offset_of!(YelangMacroDescriptor, invoke), 24);
    } else {
        assert!(
            offset_of!(YelangMacroDescriptor, name) < offset_of!(YelangMacroDescriptor, name_len)
        );
        assert!(
            offset_of!(YelangMacroDescriptor, name_len) < offset_of!(YelangMacroDescriptor, kind)
        );
        assert!(
            offset_of!(YelangMacroDescriptor, kind) < offset_of!(YelangMacroDescriptor, invoke)
        );
    }
}

#[test]
fn invoke_layout_64bit() {
    assert_eq!(align_of::<YelangMacroInvoke>(), 8);

    if IS_64BIT {
        assert_eq!(size_of::<YelangMacroInvoke>(), 24);
        assert_eq!(offset_of!(YelangMacroInvoke, fn_like), 0);
        assert_eq!(offset_of!(YelangMacroInvoke, attr), 8);
        assert_eq!(offset_of!(YelangMacroInvoke, derive), 16);
    } else {
        assert!(offset_of!(YelangMacroInvoke, fn_like) < offset_of!(YelangMacroInvoke, attr));
        assert!(offset_of!(YelangMacroInvoke, attr) < offset_of!(YelangMacroInvoke, derive));
    }
}

#[test]
fn kind_layout_and_discriminants() {
    assert_eq!(align_of::<YelangProcMacroKind>(), 4);
    assert_eq!(size_of::<YelangProcMacroKind>(), 4);

    // `#[repr(C)]` enum with no explicit discriminants uses 0, 1, 2.
    assert_eq!(YelangProcMacroKind::FunctionLike as i32, 0);
    assert_eq!(YelangProcMacroKind::Attribute as i32, 1);
    assert_eq!(YelangProcMacroKind::Derive as i32, 2);
}

#[test]
fn function_pointer_types_are_pointer_sized() {
    use yelang_proc_macro_bridge::abi::{
        YelangAllocFn, YelangAttrMacro, YelangDeriveMacro, YelangFnLikeMacro, YelangFreeFn,
        YelangProcMacroEntry,
    };

    assert_eq!(size_of::<YelangFnLikeMacro>(), size_of::<usize>());
    assert_eq!(size_of::<YelangAttrMacro>(), size_of::<usize>());
    assert_eq!(size_of::<YelangDeriveMacro>(), size_of::<usize>());
    assert_eq!(size_of::<YelangAllocFn>(), size_of::<usize>());
    assert_eq!(size_of::<YelangFreeFn>(), size_of::<usize>());
    assert_eq!(size_of::<YelangProcMacroEntry>(), size_of::<usize>());
}
