use gtk4::{gio, prelude::*, Application, ApplicationWindow};
use piperchat::window::Window;

const APP_ID: &str = "org.gtk_rs.HelloWorld1";

fn main() {
    // Register and include resources
    gio::resources_register_include!("resources.gresource").expect("Failed to register resources.");

    // Create a new application
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(build_ui);

    // Run the application
    app.run();
}

fn build_ui(app: &Application) {
    // Create a window and set the title
    let window = Window::new(app);
    window.present();
}
