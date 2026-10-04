//! Definition-only specialization: parsed tables are supplied by the G0 caller.
use super::*;
pub(super) const ROOTS: &[&str] = &[
    "control-domain",
    "validator-program-names",
    "validator-program-references",
    "validator-program-callcycles-fast",
    "validator-program-schemas",
    "validator-program-schema-types",
    "validator-program-port-types",
    "control-compile",
];
pub(super) fn graphs(definitions: &[Graph]) -> Vec<Graph> {
    let table = SemanticType::Slice(Box::new(SemanticType::Slice(Box::new(int()))));
    // Only the complete contextual pipeline uses this specialization. Its
    // global port-type proof checks the same locations, plus schema references.
    // Standalone domain graphs retain every type check.
    let scope: Vec<_> = definitions
        .iter()
        .filter(|g| g.name.starts_with("control-domain"))
        .cloned()
        .collect();
    let mut specialized =
        super::control_fast::variants(&scope, &[("validator-type-profile", "context-type-proven")]);
    let mut proven = G::new(
        "context-type-proven",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let yes = proven.bool(true);
    let mut result = vec![proven.finish(vec![yes])];
    let specialized_root = specialized
        .iter()
        .find(|g| g.name == "control-domain-fast")
        .expect("specialized domain")
        .clone();
    specialized.retain(|g| g.name != "control-domain-fast");
    result.extend(specialized);
    result.extend(ROOTS.iter().map(|name| {
        let mut graph = if *name == "control-domain" { specialized_root.clone() }
            else { definitions.iter().find(|g| g.name == *name).expect("context root").clone() };
        graph.name = format!("{name}-context");
        for id in [1,2] {
            graph.inputs.push(port(id, table.clone()));
        }
        let removed: std::collections::BTreeMap<_, _> = graph.nodes.iter().filter_map(|node| {
            let Operation::Subgraph(callee) = &node.operation else { return None };
            let input = match callee.as_str() {
                "reader-program-ast" => 1,
                "reader-program-schemas" => 2,
                _ => return None,
            };
            assert_eq!(node.outputs.len(), 1);
            Some((node.id, input))
        }).collect();
        assert!(!removed.is_empty(), "root {name} must parse a table");
        graph.nodes.retain(|node| !removed.contains_key(&node.id));
        graph.edges.retain(|edge| !matches!(edge.to, TargetEndpoint::NodeInput{node,..} if removed.contains_key(&node)));
        for edge in &mut graph.edges {
            if let SourceEndpoint::NodeOutput{node,port:0} = edge.from
                && let Some(input) = removed.get(&node) {
                edge.from = SourceEndpoint::GraphInput(*input);
            }
        }
        graph
    }));
    result
}
