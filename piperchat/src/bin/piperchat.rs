use adw::prelude::*;
use gtk::gio;
use piperchat::{gui::window::Window, APP_ID};

fn main() {
    // Register and include resources
    gio::resources_register_include!("piperchat.gresource").expect("Failed to register resources.");

    // Create a new application
    let app = adw::Application::builder().application_id(APP_ID).build();

    // Connect signals
    app.connect_startup(setup_shortcuts);
    app.connect_activate(build_ui);

    // Run the application
    app.run();
}

fn build_ui(app: &adw::Application) {
    // Create a new custom window and show it
    let window = Window::new(app);
    window.present();
}

fn setup_shortcuts(_app: &adw::Application) {
    // app.set_accels_for_action("win.filter('All')", &["<Ctrl>a"]);
    // app.set_accels_for_action("win.filter('Open')", &["<Ctrl>o"]);
    // app.set_accels_for_action("win.filter('Done')", &["<Ctrl>d"]);
}
