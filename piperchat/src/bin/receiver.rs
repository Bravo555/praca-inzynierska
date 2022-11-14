use gst::prelude::*;

fn main() {
    gst::init().unwrap();

    let source = gst::ElementFactory::make("udpsrc")
        .property_from_str("port", "1234")
        .build()
        .unwrap();
    let depacket = gst::ElementFactory::make("rtph264depay").build().unwrap();
    let decode = gst::ElementFactory::make("avdec_h264").build().unwrap();
    let convert = gst::ElementFactory::make("videoconvert").build().unwrap();
    let sink = gst::ElementFactory::make("autovideosink").build().unwrap();

    let pipeline = gst::Pipeline::builder().name("pipeline").build();

    pipeline
        .add_many(&[&source, &depacket, &decode, &convert, &sink])
        .unwrap();

    let caps = gst::Caps::new_simple("application/x-rtp", &[]);
    gst::Element::link_filtered(&source, &depacket, &caps).unwrap();

    gst::Element::link_many(&[&depacket, &decode, &convert, &sink])
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
                        println!("STARTED RECEIVING VIDEO FROM UDP PORT 1234");
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

    pipeline.set_state(gst::State::Null).unwrap();
}
