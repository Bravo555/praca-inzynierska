use gst::prelude::*;

fn main() {
    gst::init().unwrap();

    let source = gst::ElementFactory::make("v4l2src").build().unwrap();
    let convert = gst::ElementFactory::make("videoconvert").build().unwrap();
    let encode = gst::ElementFactory::make("x264enc")
        .property_from_str("pass", "qual")
        .property_from_str("quantizer", "20")
        .property_from_str("tune", "zerolatency")
        .build()
        .unwrap();
    let packet = gst::ElementFactory::make("rtph264pay").build().unwrap();
    let sink = gst::ElementFactory::make("udpsink")
        .property_from_str("host", "127.0.0.1")
        .property_from_str("port", "1234")
        .build()
        .unwrap();

    let pipeline = gst::Pipeline::builder().name("pipeline").build();

    pipeline
        .add_many(&[&source, &convert, &encode, &packet, &sink])
        .unwrap();
    gst::Element::link_many(&[&source, &convert, &encode, &packet, &sink])
        .expect("Elements could not be linked.");

    pipeline
        .set_state(gst::State::Playing)
        .expect("Unable to set the playbin to the `Playing` state");

    let bus = pipeline.bus().unwrap();

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        match msg.view() {
            // This is called when an End-Of-Stream message is posted on the bus.
            // We just set the pipeline to READY (which stops playback).
            gst::MessageView::Eos(..) => {
                println!("End-Of-Stream reached.");
                pipeline
                    .set_state(gst::State::Null)
                    .expect("Unable to set the pipeline to the `Ready` state");
                break;
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

                    if new_state == gst::State::Playing {
                        println!("STARTED SENDING VIDEO ON UDP PORT 1234");
                    }

                    println!(
                        "Pipeline state changed from {:?} to {:?}",
                        old_state, new_state
                    );

                    if new_state == gst::State::Playing {
                        println!("Source capabilities in {new_state:?} state:");
                        let pad = source.static_pad("src").unwrap();
                        let caps = pad.caps().unwrap_or_else(|| pad.query_caps(None));
                        println!("{:#?}", &caps);
                    }
                }
            }
            _ => (),
        }
    }
}
