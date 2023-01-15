mod imp;

use glib::Object;
use gtk::glib;

glib::wrapper! {
    pub struct ContactObject(ObjectSubclass<imp::ContactObject>);
}

impl ContactObject {
    pub fn new(name: String) -> Self {
        Object::builder().property("name", name).build()
    }
}

#[derive(Default)]
pub struct ContactData {
    pub name: String,
}
