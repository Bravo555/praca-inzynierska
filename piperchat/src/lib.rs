pub mod custom_button;
pub mod utils;
pub mod window;

pub mod task_object;
pub mod task_row;

pub use custom_button::*;
pub use task_object::*;
pub use task_row::*;
pub use window::*;

pub const APP_ID: &str = "org.gtk_rs.Todo2";
