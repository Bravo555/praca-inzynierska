use std::cell::RefCell;
use std::rc::Rc;

use glib::once_cell::sync::Lazy;
use glib::{ParamSpec, ParamSpecString, Value};
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use super::ContactData;

// Object holding the state
#[derive(Default)]
pub struct ContactObject {
    pub data: Rc<RefCell<ContactData>>,
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for ContactObject {
    const NAME: &'static str = "ContactObject";
    type Type = super::ContactObject;
}

// Trait shared by all GObjects
impl ObjectImpl for ContactObject {
    fn properties() -> &'static [ParamSpec] {
        static PROPERTIES: Lazy<Vec<ParamSpec>> =
            Lazy::new(|| vec![ParamSpecString::builder("name").build()]);
        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
        match pspec.name() {
            "name" => {
                let input_value = value.get().expect("The value needs to be of type `bool`.");
                self.data.borrow_mut().name = input_value;
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
        match pspec.name() {
            "name" => self.data.borrow().name.to_value(),
            _ => unimplemented!(),
        }
    }
}
