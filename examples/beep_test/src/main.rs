use std::time::{Duration, Instant};

use firewheel_cpal::{factory_nodes::beep_test::BeepTestNode, InactiveFwCpalCtx, UpdateStatus};

const BEEP_DURATION: Duration = Duration::from_secs(4);
const UPDATE_INTERVAL: Duration = Duration::from_millis(15);

fn main() {
    simple_log::quick!("info");

    println!("Firewheel beep test...");

    let mut cx = InactiveFwCpalCtx::new(Default::default());

    let graph = cx.cx_mut().graph_mut();
    let beep_test_node = graph.add_node(0, 2, BeepTestNode::new(440.0, -16.0));
    graph
        .add_edge(beep_test_node, 0, graph.graph_out_node(), 0, false)
        .unwrap();
    graph
        .add_edge(beep_test_node, 1, graph.graph_out_node(), 1, false)
        .unwrap();

    let mut active_cx = Some(cx.activate(None, true).unwrap());

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
            UpdateStatus::Deactivated { cx: _, error_msg } => {
                log::error!("Deactivated unexpectedly: {:?}", error_msg);

                break;
            }
        }
    }

    println!("finished");
}
