//! The JIT backend: owns the Cranelift JIT module and turns bytecode into
//! callable native machine code.
//!
//! The backend holds a single [`JITModule`] for its lifetime. Every compiled
//! function is defined and finalized inside that module, and the returned
//! function pointers stay valid for as long as the backend (and therefore the
//! module) is alive.

use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_codegen::ir::{types, AbiParam};
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::Context;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use yelang_vm::{CompiledFunction, CompiledProgram, Value, Vm};

use crate::compile::{build_function, is_jittable, JitError};
use crate::profiling::Profiler;

/// Manages the Cranelift JIT context and the flying-start profiler.
pub struct JitBackend {
    module: JITModule,
    profiler: Profiler,
    /// Monotonic counter used to give each compiled function a unique symbol.
    next_id: u64,
}

impl JitBackend {
    /// Create a backend targeting the host architecture.
    pub fn new() -> Result<Self, JitError> {
        Self::with_profiler(Profiler::new())
    }

    /// Create a backend with a custom profiler (e.g. a tuned threshold).
    pub fn with_profiler(profiler: Profiler) -> Result<Self, JitError> {
        let mut flag_builder = cranelift::codegen::settings::builder();
        // JITModule cannot resolve colocated libcalls; disable them.
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| JitError::Codegen(e.to_string()))?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| JitError::Codegen(e.to_string()))?;
        let flags = cranelift::codegen::settings::Flags::new(flag_builder);

        let isa_builder =
            cranelift_native::builder().map_err(|e| JitError::Codegen(e.to_string()))?;
        let isa = isa_builder
            .finish(flags)
            .map_err(|e| JitError::Codegen(e.to_string()))?;

        let builder = JITBuilder::with_isa(isa, default_libcall_names());
        let module = JITModule::new(builder);

        Ok(Self {
            module,
            profiler,
            next_id: 0,
        })
    }

    /// Shared reference to the flying-start profiler.
    pub fn profiler(&self) -> &Profiler {
        &self.profiler
    }

    /// Mutable reference to the flying-start profiler.
    pub fn profiler_mut(&mut self) -> &mut Profiler {
        &mut self.profiler
    }

    /// Whether `func` can be lowered to native code by this backend.
    pub fn is_jittable(&self, func: &CompiledFunction) -> bool {
        is_jittable(func)
    }

    /// Compile `bytecode` to native machine code and return a function
    /// pointer to it.
    ///
    /// The native ABI is `extern "C" fn(arg0: i64, …, argN: i64) -> i64` with
    /// `num_args` parameters. The pointer is valid for the lifetime of this
    /// backend.
    ///
    /// Returns [`JitError::Unsupported`] if the bytecode falls outside the
    /// JIT-able subset; callers should fall back to the interpreter in that
    /// case (see [`JitBackend::execute`]).
    pub fn compile_function(&mut self, bytecode: &CompiledFunction) -> Result<*const u8, JitError> {
        if !is_jittable(bytecode) {
            return Err(JitError::Unsupported(
                "function uses instructions outside the JIT subset".into(),
            ));
        }

        // Signature: num_args × i64 -> i64.
        let mut sig = self.module.make_signature();
        for _ in 0..bytecode.num_args {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let name = format!("yelang_jit_fn_{}", self.next_id);
        self.next_id += 1;

        let func_id = self
            .module
            .declare_function(&name, Linkage::Export, &sig)
            .map_err(|e| JitError::Module(e.to_string()))?;

        let mut ctx: Context = self.module.make_context();
        ctx.func.signature = sig;

        let frontend_config = self.module.target_config();
        {
            let mut fb_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
            build_function(bytecode, &mut builder)?;
            builder.finalize(frontend_config);
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| JitError::Module(e.to_string()))?;
        self.module.clear_context(&mut ctx);

        let _ = self
            .module
            .finalize_definitions()
            .map_err(|e| JitError::Module(e.to_string()))?;

        Ok(self.module.get_finalized_function(func_id))
    }

    /// Compile (if possible) and execute `bytecode`, returning its result.
    ///
    /// Zero-argument functions in the JIT subset are compiled and run natively.
    /// Everything else — functions with parameters, or functions using
    /// unsupported instructions — is executed on the interpreter, preserving
    /// the flying-start guarantee that execution always makes progress.
    pub fn execute(&mut self, bytecode: &CompiledFunction) -> Result<Value, JitError> {
        if is_jittable(bytecode) && bytecode.num_args == 0 {
            let ptr = self.compile_function(bytecode)?;
            // SAFETY: the pointer came from `compile_function` above and the
            // backing module lives in `self`; the signature matches the
            // zero-argument ABI we compiled for.
            let result = unsafe {
                let f: extern "C" fn() -> i64 = std::mem::transmute(ptr);
                f()
            };
            return Ok(Value::Int(i128::from(result)));
        }

        interpret(bytecode)
    }

    /// Compile and execute `bytecode` natively with the supplied `i64`
    /// arguments.
    ///
    /// Unlike [`JitBackend::execute`], this always takes the native path and
    /// therefore requires the function to be JIT-able with
    /// `args.len() == num_args`. Arity 0–6 is supported directly.
    pub fn execute_with_args(
        &mut self,
        bytecode: &CompiledFunction,
        args: &[i64],
    ) -> Result<Value, JitError> {
        if !is_jittable(bytecode) {
            return Err(JitError::Unsupported(
                "function uses instructions outside the JIT subset".into(),
            ));
        }
        if args.len() != bytecode.num_args as usize {
            return Err(JitError::Unsupported(format!(
                "expected {} argument(s), got {}",
                bytecode.num_args,
                args.len()
            )));
        }

        let ptr = self.compile_function(bytecode)?;
        let result = call_native(ptr, args);
        Ok(Value::Int(i128::from(result)))
    }
}

/// Invoke a compiled native function pointer with 0–6 `i64` arguments.
///
/// # Safety (internal)
/// The `unsafe` blocks below are sound because `ptr` is required to point at
/// machine code compiled with the matching `extern "C" fn(i64 × N) -> i64`
/// ABI; `execute_with_args` enforces the arity against `num_args` before
/// calling, and the backing module outlives the call.
fn call_native(ptr: *const u8, args: &[i64]) -> i64 {
    match args.len() {
        0 => unsafe {
            let f: extern "C" fn() -> i64 = std::mem::transmute(ptr);
            f()
        },
        1 => unsafe {
            let f: extern "C" fn(i64) -> i64 = std::mem::transmute(ptr);
            f(args[0])
        },
        2 => unsafe {
            let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(ptr);
            f(args[0], args[1])
        },
        3 => unsafe {
            let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(ptr);
            f(args[0], args[1], args[2])
        },
        4 => unsafe {
            let f: extern "C" fn(i64, i64, i64, i64) -> i64 = std::mem::transmute(ptr);
            f(args[0], args[1], args[2], args[3])
        },
        5 => unsafe {
            let f: extern "C" fn(i64, i64, i64, i64, i64) -> i64 = std::mem::transmute(ptr);
            f(args[0], args[1], args[2], args[3], args[4])
        },
        6 => unsafe {
            let f: extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 = std::mem::transmute(ptr);
            f(args[0], args[1], args[2], args[3], args[4], args[5])
        },
        n => panic!("execute_with_args supports at most 6 arguments, got {n}"),
    }
}

/// Run a single function on the interpreter (the cold-path fallback).
///
/// Wraps the function as the entry point of a one-function program so the
/// existing [`Vm`] can execute it unchanged.
fn interpret(func: &CompiledFunction) -> Result<Value, JitError> {
    let mut program = CompiledProgram::new();
    let id = program.add_function(func.clone());
    program.entry = Some(id);
    let mut vm = Vm::new();
    vm.execute(&program)
        .map_err(|e| JitError::Interpret(e.to_string()))
}
