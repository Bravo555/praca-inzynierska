mod imp;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::gio;
use gtk::glib::{self, clone, Object};

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        // Create new window
        Object::new(&[("application", app)])
    }

    fn setup_actions(&self) {
        // Create action to create new collection and add to action group "win"
        let action_set_user = gio::SimpleAction::new("set_user", None);
        action_set_user.connect_activate(clone!(@weak self as window => move |_, _| {
            window.imp().stack.set_visible_child_name("main");
        }));
        self.add_action(&action_set_user);
    }
}
