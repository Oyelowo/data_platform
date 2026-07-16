/*!
 * Yelang Proc-Macro Server
 *
 * Executable entry point. Loads proc-macro dynamic libraries and executes macro
 * expansion requests from the compiler over framed stdin/stdout messages.
 */

fn main() {
    // Implemented in the server module.
    yelang_proc_macro_server::server::run();
}
