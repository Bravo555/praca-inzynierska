use gst::{glib, prelude::*};
use gst_video::prelude::*;
use std::os::raw::c_void;
use std::process;

use gtk::prelude::*;

use std::ops;

// Custom struct to keep our window reference alive and to store the timeout id so that we can remove it from the main
// context again later and drop the references it keeps inside its closures
struct AppWindow {
    main_window: gtk::Window,
    timeout_id: Option<glib::SourceId>,
}

impl ops::Deref for AppWindow {
    type Target = gtk::Window;

    fn deref(&self) -> &gtk::Window {
        &self.main_window
    }
}

impl Drop for AppWindow {
    fn drop(&mut self) {
        if let Some(source_id) = self.timeout_id.take() {
            source_id.remove();
        }
    }
}

// Extract tags from streams of @stype and add the info in the UI.
fn add_streams_info(sink: &gst::Element, textbuf: &gtk::TextBuffer, stype: &str) {
    let propname: &str = &format!("n-{}", stype);
    let signame: &str = &format!("get-{}-tags", stype);

    let x = sink.property::<i32>(propname);
    for i in 0..x {
        let tags = sink.emit_by_name::<Option<gst::TagList>>(signame, &[&i]);

        if let Some(tags) = tags {
            textbuf.insert_at_cursor(&format!("{} stream {}:\n ", stype, i));

            if let Some(codec) = tags.get::<gst::tags::VideoCodec>() {
                textbuf.insert_at_cursor(&format!("    codec: {} \n", codec.get()));
            }

            if let Some(codec) = tags.get::<gst::tags::AudioCodec>() {
                textbuf.insert_at_cursor(&format!("    codec: {} \n", codec.get()));
            }

            if let Some(lang) = tags.get::<gst::tags::LanguageCode>() {
                textbuf.insert_at_cursor(&format!("    language: {} \n", lang.get()));
            }

            if let Some(bitrate) = tags.get::<gst::tags::Bitrate>() {
                textbuf.insert_at_cursor(&format!("    bitrate: {} \n", bitrate.get()));
            }
        }
    }
}

// Extract metadata from all the streams and write it to the text widget in the GUI
fn analyze_streams(playbin: &gst::Element, textbuf: &gtk::TextBuffer) {
    {
        textbuf.set_text("");
    }

    add_streams_info(playbin, textbuf, "video");
    add_streams_info(playbin, textbuf, "audio");
    add_streams_info(playbin, textbuf, "text");
}

// This creates all the GTK+ widgets that compose our application, and registers the callbacks
fn create_ui(pipeline_: &gst::Pipeline, sink: &gst::Element) -> AppWindow {
    let main_window = gtk::Window::new(gtk::WindowType::Toplevel);
    main_window.connect_delete_event(|_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    let play_button =
        gtk::Button::from_icon_name(Some("media-playback-start"), gtk::IconSize::SmallToolbar);
    let pipeline = pipeline_.clone();
    play_button.connect_clicked(move |_| {
        let pipeline = &pipeline;
        pipeline
            .set_state(gst::State::Playing)
            .expect("Unable to set the pipeline to the `Playing` state");
    });

    let pause_button =
        gtk::Button::from_icon_name(Some("media-playback-pause"), gtk::IconSize::SmallToolbar);
    let pipeline = pipeline_.clone();
    pause_button.connect_clicked(move |_| {
        let pipeline = &pipeline;
        pipeline
            .set_state(gst::State::Paused)
            .expect("Unable to set the pipeline to the `Paused` state");
    });

    let stop_button =
        gtk::Button::from_icon_name(Some("media-playback-stop"), gtk::IconSize::SmallToolbar);
    let pipeline = pipeline_.clone();
    stop_button.connect_clicked(move |_| {
        let pipeline = &pipeline;
        pipeline
            .set_state(gst::State::Ready)
            .expect("Unable to set the pipeline to the `Ready` state");
    });

    let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    let pipeline = pipeline_.clone();
    let slider_update_signal_id = slider.connect_value_changed(move |slider| {
        let pipeline = &pipeline;
        let value = slider.value() as u64;
        if pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                value * gst::ClockTime::SECOND,
            )
            .is_err()
        {
            eprintln!("Seeking to {} failed", value);
        }
    });

    slider.set_draw_value(false);
    let pipeline = pipeline_.clone();
    let lslider = slider.clone();
    // Update the UI (seekbar) every second
    let timeout_id = glib::timeout_add_seconds_local(1, move || {
        let pipeline = &pipeline;
        let lslider = &lslider;

        if let Some(dur) = pipeline.query_duration::<gst::ClockTime>() {
            lslider.set_range(0.0, dur.seconds() as f64);

            if let Some(pos) = pipeline.query_position::<gst::ClockTime>() {
                lslider.block_signal(&slider_update_signal_id);
                lslider.set_value(pos.seconds() as f64);
                lslider.unblock_signal(&slider_update_signal_id);
            }
        }

        Continue(true)
    });

    let video_window = gtk::DrawingArea::new();

    let video_overlay = sink
        .clone()
        .dynamic_cast::<gst_video::VideoOverlay>()
        .unwrap();

    video_window.connect_realize(move |video_window| {
        let video_overlay = &video_overlay;
        let gdk_window = video_window.window().unwrap();

        if !gdk_window.ensure_native() {
            println!("Can't create native window for widget");
            process::exit(-1);
        }

        let display_type_name = gdk_window.display().type_().name();
        if display_type_name == "GdkX11Display" {
            extern "C" {
                pub fn gdk_x11_window_get_xid(
                    window: *mut glib::gobject_ffi::GObject,
                ) -> *mut c_void;
            }

            #[allow(clippy::cast_ptr_alignment)]
            unsafe {
                let xid = gdk_x11_window_get_xid(gdk_window.as_ptr() as *mut _);
                video_overlay.set_window_handle(xid as usize);
            }
        } else {
            println!("Add support for display type '{}'", display_type_name);
            process::exit(-1);
        }
    });

    let streams_list = gtk::TextView::new();
    streams_list.set_editable(false);
    let pipeline_weak = pipeline_.downgrade();
    let streams_list_weak = glib::SendWeakRef::from(streams_list.downgrade());
    let bus = pipeline_.bus().unwrap();

    #[allow(clippy::single_match)]
    bus.connect_message(Some("application"), move |_, msg| match msg.view() {
        gst::MessageView::Application(application) => {
            println!("RECEIVED MESSAGE");
            let pipeline = match pipeline_weak.upgrade() {
                Some(pipeline) => pipeline,
                None => return,
            };

            let streams_list = match streams_list_weak.upgrade() {
                Some(streams_list) => streams_list,
                None => return,
            };

            if application.structure().map(|s| s.name()) == Some("video-format") {
                println!("RECEIVED VIDEO FORMAT MESSAGE");
                let textbuf = streams_list
                    .buffer()
                    .expect("Couldn't get buffer from text_view");
                let structure = application.structure().unwrap();
                let caps: gst::Caps = structure.get("video").unwrap();
                textbuf.set_text(&format!("{caps:#?}"));
            }
        }
        _ => unreachable!(),
    });

    let vbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    vbox.pack_start(&video_window, true, true, 0);
    vbox.pack_start(&streams_list, false, false, 2);

    main_window.add(&vbox);
    main_window.set_default_size(640, 480);

    main_window.show_all();

    AppWindow {
        main_window,
        timeout_id: Some(timeout_id),
    }
}

// We are possibly in a GStreamer working thread, so we notify the main
// thread of this event through a message in the bus
fn post_app_message(element: &gst::Element, caps: &gst::Caps) {
    element
        .post_message(gst::message::Application::new(gst::Structure::new(
            "video-format",
            &[("video", &caps)],
        )))
        .unwrap();
}

fn main() {
    // Initialize GTK
    if let Err(err) = gtk::init() {
        eprintln!("Failed to initialize GTK: {}", err);
        return;
    }

    // Initialize GStreamer
    if let Err(err) = gst::init() {
        eprintln!("Failed to initialize Gst: {}", err);
        return;
    }

    let source = gst::ElementFactory::make("v4l2src").build().unwrap();
    let convert = gst::ElementFactory::make("videoconvert").build().unwrap();
    let sink = gst::ElementFactory::make("xvimagesink").build().unwrap();

    let pipeline = gst::Pipeline::builder().name("pipeline").build();

    pipeline.add_many(&[&source, &convert, &sink]).unwrap();
    gst::Element::link_many(&[&source, &convert, &sink]).expect("Elements could not be linked.");

    let window = create_ui(&pipeline, &sink);

    let bus = pipeline.bus().unwrap();
    bus.add_signal_watch();

    let pipeline_weak = pipeline.downgrade();
    bus.connect_message(None, move |_, msg| {
        let pipeline = match pipeline_weak.upgrade() {
            Some(pipeline) => pipeline,
            None => return,
        };

        match msg.view() {
            // This is called when an End-Of-Stream message is posted on the bus.
            // We just set the pipeline to READY (which stops playback).
            gst::MessageView::Eos(..) => {
                println!("End-Of-Stream reached.");
                pipeline
                    .set_state(gst::State::Ready)
                    .expect("Unable to set the pipeline to the `Ready` state");
            }

            // This is called when an error message is posted on the bus
            gst::MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
            }
            // This is called when the pipeline changes states. We use it to
            // keep track of the current state.
            gst::MessageView::StateChanged(state_changed) => {
                if state_changed.src().map(|s| s == pipeline).unwrap_or(false) {
                    let new_state = state_changed.current();
                    let old_state = state_changed.old();

                    println!(
                        "Pipeline state changed from {:?} to {:?}",
                        old_state, new_state
                    );

                    if new_state == gst::State::Playing {
                        println!("Source capabilities in {new_state:?} state:");
                        let pad = source.static_pad("src").unwrap();
                        let caps = pad.caps().unwrap_or_else(|| pad.query_caps(None));
                        println!("{:#?}", &caps);
                        post_app_message(&source, &caps);
                    }
                }
            }
            _ => (),
        }
    });

    pipeline
        .set_state(gst::State::Playing)
        .expect("Unable to set the playbin to the `Playing` state");

    gtk::main();
    window.hide();
    pipeline
        .set_state(gst::State::Null)
        .expect("Unable to set the playbin to the `Null` state");

    bus.remove_signal_watch();
}
