mod imp;

use crate::{GuiEvent, VideoPreference};
use adw::subclass::prelude::*;
use adw::{prelude::*, ActionRow, ResponseAppearance};
use async_std::channel::Sender;
use futures::{select, FutureExt};
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
    pub fn new(app: &adw::Application, gui_tx: Sender<GuiEvent>) -> Self {
        // Create new window
        let window: Window = Object::new(&[("application", app)]);
        window.imp().window_data.borrow_mut().gui_tx = Some(gui_tx);
        window
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
            .window_data
            .borrow()
            .contacts
            .clone()
            .expect("Could not get current tasks.")
    }

    fn sender(&self) -> Sender<GuiEvent> {
        self.imp()
            .window_data
            .borrow()
            .gui_tx
            .as_ref()
            .unwrap()
            .clone()
    }

    fn setup_contacts(&self) {
        // Create new model
        let model = gio::ListStore::new(ContactObject::static_type());

        // Get state and set model
        self.imp().window_data.borrow_mut().contacts = Some(model);

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

        call_button.connect_clicked(
            clone!(@weak contact_object, @weak self as window => move |_button| {
                let id = contact_object.property::<u32>("id");
                let name = contact_object.property::<String>("name");
                println!("calling {name}");
                window.sender().send_blocking(GuiEvent::CallStart(id, name)).unwrap();

            }),
        );

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

    pub fn set_contacts(&self, contacts: Vec<(u32, String)>) {
        let contact_list = self.contacts();
        contact_list.remove_all();
        contacts
            .into_iter()
            .map(|(id, name)| ContactObject::new(id, name))
            .for_each(|contact| {
                contact_list.append(&contact);
            });
    }

    pub fn accept_call(&self) {
        self.sender()
            .send_blocking(GuiEvent::CallAccepted(VideoPreference::Enabled))
            .unwrap();
    }

    pub fn accept_call_without_video(&self) {
        self.sender()
            .send_blocking(GuiEvent::CallAccepted(VideoPreference::Disabled))
            .unwrap();
    }

    pub fn reject_call(&self) {
        self.sender().send_blocking(GuiEvent::CallRejected).unwrap();
    }
}

#[derive(Default)]
pub struct WindowData {
    pub contacts: Option<gio::ListStore>,
    pub username: String,
    pub gui_tx: Option<Sender<GuiEvent>>,
}
