use g0::{bootstrap_compiler::*, gir::Operation};
use std::collections::BTreeMap;

#[test]
fn compiler_callback_graph_stays_within_host_depth_budget() {
    let document = compiler_document();
    let references: BTreeMap<_, Vec<&str>> = document
        .graphs
        .iter()
        .map(|graph| {
            let references = graph
                .nodes
                .iter()
                .flat_map(|node| match &node.operation {
                    Operation::Subgraph(body) | Operation::Map { body } => vec![body.as_str()],
                    Operation::Select {
                        when_true,
                        when_false,
                    } => vec![when_true.as_str(), when_false.as_str()],
                    Operation::Loop {
                        condition, body, ..
                    } => vec![condition.as_str(), body.as_str()],
                    Operation::Match { arms, default } => arms
                        .iter()
                        .map(|a| a.graph.as_str())
                        .chain(std::iter::once(default.as_str()))
                        .collect(),
                    _ => vec![],
                })
                .collect();
            (graph.name.as_str(), references)
        })
        .collect();
    let mut depths = BTreeMap::<&str, usize>::new();
    for _ in 0..references.len() {
        for (name, targets) in &references {
            if targets.iter().all(|target| depths.contains_key(target)) {
                let depth = targets
                    .iter()
                    .map(|target| depths[target])
                    .max()
                    .unwrap_or(0)
                    + 1;
                depths.insert(name, depth);
            }
        }
    }
    assert_eq!(
        depths.len(),
        references.len(),
        "compiler callback graph must be closed and acyclic"
    );
    let maximum = *depths.values().max().unwrap();
    eprintln!(
        "compiler callback DAG depth: {maximum}; compile entry depth: {}",
        depths["compile"]
    );
    assert!(maximum <= compiler_limits().max_call_depth);
}
