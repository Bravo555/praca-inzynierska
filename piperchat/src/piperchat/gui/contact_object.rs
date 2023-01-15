mod imp;

use glib::Object;
use gtk::glib;

glib::wrapper! {
    pub struct ContactObject(ObjectSubclass<imp::ContactObject>);
}

impl ContactObject {
    pub fn new(id: u32, name: String) -> Self {
        Object::builder()
            .property("id", id)
            .property("name", name)
            .build()
    }
}

#[derive(Default)]
pub struct ContactData {
    pub id: u32,
    pub name: String,
}
