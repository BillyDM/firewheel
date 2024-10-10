use std::time::{Duration, Instant};

use firewheel::{basic_nodes::beep_test::BeepTestNode, InactiveCtx, UpdateStatus};

const BEEP_FREQUENCY_HZ: f32 = 440.0;
const BEEP_GAIN_DB: f32 = -18.0;
const BEEP_DURATION: Duration = Duration::from_secs(4);
const UPDATE_INTERVAL: Duration = Duration::from_millis(15);

fn main() {
    simple_log::quick!("info");

    println!("Firewheel beep test...");

    let mut cx = InactiveCtx::new(Default::default());

    let graph = cx.graph_mut();
    let beep_test_node = graph.add_node(0, 2, BeepTestNode::new(BEEP_FREQUENCY_HZ, BEEP_GAIN_DB));
    graph
        .add_edge(beep_test_node, 0, graph.graph_out_node(), 0, false)
        .unwrap();
    graph
        .add_edge(beep_test_node, 1, graph.graph_out_node(), 1, false)
        .unwrap();

    let mut active_cx = Some(cx.activate(None, true, ()).unwrap());

    let start = Instant::now();
    while start.elapsed() < BEEP_DURATION {
        std::thread::sleep(UPDATE_INTERVAL);

        match active_cx.take().unwrap().update() {
            UpdateStatus::Ok { cx, graph_error } => {
                active_cx = Some(cx);

                if let Some(e) = graph_error {
                    log::error!("{}", e);
                }
            }
            UpdateStatus::Deactivated { error_msg, .. } => {
                log::error!("Deactivated unexpectedly: {:?}", error_msg);

                break;
            }
        }
    }

    println!("finished");
}
