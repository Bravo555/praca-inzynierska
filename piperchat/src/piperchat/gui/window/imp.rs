use std::cell::RefCell;

use adw::{prelude::*, subclass::prelude::*, EntryRow};
use glib::subclass::InitializingObject;
use gtk::{gio, glib, CompositeTemplate, Entry, ListBox, Stack};

// Object holding the state
#[derive(CompositeTemplate, Default)]
#[template(resource = "/eu/mguzik/piperchat/window.ui")]
pub struct Window {
    #[template_child]
    pub stack: TemplateChild<Stack>,
    #[template_child]
    pub stack_name_entry: TemplateChild<Entry>,
    #[template_child]
    pub name_entry: TemplateChild<EntryRow>,
    #[template_child]
    pub contacts_list: TemplateChild<ListBox>,
    pub contacts: RefCell<Option<gio::ListStore>>,
    pub username: RefCell<String>,
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for Window {
    // `NAME` needs to match `class` attribute of template
    const NAME: &'static str = "PiperchatWindow";
    type Type = super::Window;
    type ParentType = adw::ApplicationWindow;

    fn class_init(class: &mut Self::Class) {
        class.bind_template();
        class.bind_template_callbacks();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

#[gtk::template_callbacks]
impl Window {
    #[template_callback]
    fn handle_start(&self) {
        let username = self.stack_name_entry.text();
        println!("{}", username.as_str());
        *self.username.borrow_mut() = String::from(username.as_str());
        self.name_entry.set_text(username.as_str());
    }
}

// Trait shared by all GObjects
impl ObjectImpl for Window {
    fn constructed(&self) {
        // Call "constructed" on parent
        self.parent_constructed();

        let obj = self.obj();
        obj.setup_actions();
        obj.setup_contacts();
    }
}

// Trait shared by all widgets
impl WidgetImpl for Window {}

// Trait shared by all windows
impl WindowImpl for Window {}

// Trait shared by all application windows
impl ApplicationWindowImpl for Window {}

impl AdwApplicationWindowImpl for Window {}
