mod imp;

use adw::subclass::prelude::*;
use adw::{prelude::*, ActionRow};
use gtk::glib::{self, clone, Object};
use gtk::{gio, Align, Button, NoSelection};

use super::contact_object::ContactObject;

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

    fn contacts(&self) -> gio::ListStore {
        // Get state
        self.imp()
            .contacts
            .borrow()
            .clone()
            .expect("Could not get current tasks.")
    }

    fn setup_contacts(&self) {
        // Create new model
        let model = gio::ListStore::new(ContactObject::static_type());

        // Get state and set model
        self.imp().contacts.replace(Some(model));

        // Wrap model with selection and pass it to the list view
        let selection_model = NoSelection::new(Some(&self.contacts()));
        self.imp().contacts_list.bind_model(Some(&selection_model),
    clone!(@weak self as window => @default-panic, move |obj| {
                let task_object = obj.downcast_ref().expect("The object should be of type `ContactObject`.");
                let row = window.create_contact_row(task_object);
                row.upcast()
            }));
    }

    fn create_contact_row(&self, contact_object: &ContactObject) -> adw::ActionRow {
        let call_button = Button::builder()
            .icon_name("call-start-symbolic")
            .valign(Align::Center)
            .css_classes(vec!["flat".into()])
            .build();

        call_button.connect_clicked(clone!(@weak contact_object => move |_button| {
            println!("calling {}", contact_object.property::<String>("name"));
        }));

        // Create row
        let row = ActionRow::builder().build();
        row.add_suffix(&call_button);

        // Bind properties
        contact_object
            .bind_property("name", &row, "title")
            .flags(glib::BindingFlags::SYNC_CREATE)
            .build();

        // Return row
        row
    }

    pub fn set_contacts(&self, contacts: Vec<(usize, String)>) {
        let contact_list = self.contacts();
        contact_list.remove_all();
        contacts
            .into_iter()
            .map(|(_id, name)| ContactObject::new(name))
            .for_each(|contact| {
                contact_list.append(&contact);
            });
    }
}
