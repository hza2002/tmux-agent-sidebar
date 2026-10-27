//! Explicit local history maintenance commands.
pub(super) fn run(args: &[String]) -> i32 {
    match crate::usage::history::command(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("usage-history: {message}");
            1
        }
    }
}
