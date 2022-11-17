use gtk4::glib;
use gtk4::subclass::prelude::*;

// Object holding the state
#[derive(Default)]
pub struct CustomButton;

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for CustomButton {
    const NAME: &'static str = "MyGtkAppCustomButton";
    type Type = super::CustomButton;
    type ParentType = gtk4::Button;
}

// Trait shared by all GObjects
impl ObjectImpl for CustomButton {}

// Trait shared by all widgets
impl WidgetImpl for CustomButton {}

// Trait shared by all buttons
impl ButtonImpl for CustomButton {}
