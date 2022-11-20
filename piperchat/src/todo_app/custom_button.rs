use glib::Object;
use gtk4::glib;

use gtk4 as gtk;

mod imp;

glib::wrapper! {
    pub struct CustomButton(ObjectSubclass<imp::CustomButton>)
        @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable,
                    gtk::Buildable, gtk::ConstraintTarget;
}

impl CustomButton {
    pub fn new() -> Self {
        Object::new(&[])
    }
}

impl Default for CustomButton {
    fn default() -> Self {
        Self::new()
    }
}
