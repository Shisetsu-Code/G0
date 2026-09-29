use crate::graph::{Graph, Op};

pub fn constant_fold(graph: &mut Graph) {
    loop {
        let snapshot = graph.nodes.clone();
        let mut changed = false;

        for node in &mut graph.nodes {
            let folded = match node.op {
                Op::Add(a, b) => bin_const(&snapshot[a].op, &snapshot[b].op, i64::checked_add),
                Op::Sub(a, b) => bin_const(&snapshot[a].op, &snapshot[b].op, i64::checked_sub),
                Op::Mul(a, b) => bin_const(&snapshot[a].op, &snapshot[b].op, i64::checked_mul),
                Op::Const(_) => None,
            };
            if let Some(value) = folded {
                node.op = Op::Const(value);
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

fn bin_const(a: &Op, b: &Op, op: fn(i64, i64) -> Option<i64>) -> Option<i64> {
    match (a, b) {
        (Op::Const(a), Op::Const(b)) => op(*a, *b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{graph::Op, parser};

    use super::*;

    #[test]
    fn folds_answer_to_42() {
        let mut graph = parser::parse(
            "g0 0.1\nconst %a i64 20\nconst %b i64 22\nadd %sum %a %b\nreturn %sum\n",
        )
        .unwrap();
        constant_fold(&mut graph);
        assert_eq!(graph.nodes[graph.output].op, Op::Const(42));
    }

    #[test]
    fn overflow_is_not_folded() {
        let mut graph = parser::parse("g0 0.1\nconst %a i64 9223372036854775807\nconst %b i64 1\nadd %sum %a %b\nreturn %sum\n").unwrap();
        constant_fold(&mut graph);
        assert!(matches!(graph.nodes[graph.output].op, Op::Add(_, _)));
    }
}
