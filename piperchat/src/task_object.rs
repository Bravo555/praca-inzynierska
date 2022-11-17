mod imp;

use glib::Object;
use gtk4::glib;

glib::wrapper! {
    pub struct TaskObject(ObjectSubclass<imp::TaskObject>);
}

impl TaskObject {
    pub fn new(completed: bool, content: String) -> Self {
        Object::new(&[("completed", &completed), ("content", &content)])
    }
}

#[derive(Default)]
pub struct TaskData {
    pub completed: bool,
    pub content: String,
}
