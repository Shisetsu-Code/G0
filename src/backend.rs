pub mod x86_64 {
    use crate::graph::{Graph, NodeId, Op};

    fn slot(id: usize) -> usize {
        (id + 1) * 8
    }

    fn topological_order(graph: &Graph) -> Vec<NodeId> {
        fn visit(id: NodeId, graph: &Graph, seen: &mut [bool], order: &mut Vec<NodeId>) {
            if seen[id] {
                return;
            }
            seen[id] = true;
            for dep in graph.nodes[id].op.inputs().into_iter().flatten() {
                visit(dep, graph, seen, order);
            }
            order.push(id);
        }

        let mut seen = vec![false; graph.nodes.len()];
        let mut order = Vec::with_capacity(graph.nodes.len());

        visit(graph.output, graph, &mut seen, &mut order);

        for id in 0..graph.nodes.len() {
            visit(id, graph, &mut seen, &mut order);
        }

        order
    }

    pub fn emit(graph: &Graph) -> String {
        let frame = ((graph.nodes.len() * 8 + 15) / 16) * 16;
        let mut out = String::new();

        out.push_str(
            ".intel_syntax noprefix\n.text\n.global g0_main\n.type g0_main, @function\ng0_main:\n",
        );
        out.push_str("    push rbp\n    mov rbp, rsp\n");

        if frame != 0 {
            out.push_str(&format!("    sub rsp, {frame}\n"));
        }

        for id in topological_order(graph) {
            let node = &graph.nodes[id];
            let dst = slot(id);

            match node.op {
                Op::Const(value) => {
                    out.push_str(&format!(
                        "    mov rax, {value}\n    mov QWORD PTR [rbp-{dst}], rax\n"
                    ));
                }
                Op::Add(a, b) => {
                    out.push_str(&format!(
                        "    mov rax, QWORD PTR [rbp-{}]\n    add rax, QWORD PTR [rbp-{}]\n    jo .Loverflow\n    mov QWORD PTR [rbp-{dst}], rax\n",
                        slot(a),
                        slot(b)
                    ));
                }
                Op::Sub(a, b) => {
                    out.push_str(&format!(
                        "    mov rax, QWORD PTR [rbp-{}]\n    sub rax, QWORD PTR [rbp-{}]\n    jo .Loverflow\n    mov QWORD PTR [rbp-{dst}], rax\n",
                        slot(a),
                        slot(b)
                    ));
                }
                Op::Mul(a, b) => {
                    out.push_str(&format!(
                        "    mov rax, QWORD PTR [rbp-{}]\n    imul rax, QWORD PTR [rbp-{}]\n    jo .Loverflow\n    mov QWORD PTR [rbp-{dst}], rax\n",
                        slot(a),
                        slot(b)
                    ));
                }
            }
        }

        out.push_str(&format!(
            "    mov rax, QWORD PTR [rbp-{}]\n    leave\n    ret\n",
            slot(graph.output)
        ));
        out.push_str(".Loverflow:\n    ud2\n");
        out.push_str(".size g0_main, .-g0_main\n.section .note.GNU-stack,\"\",@progbits\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{compile_source, parser};

    #[test]
    fn emits_x86_64_function() {
        let asm = compile_source(
            "g0 0.1\nconst %a i64 20\nconst %b i64 22\nadd %sum %a %b\nreturn %sum\n",
        )
        .unwrap();

        assert!(asm.contains(".global g0_main"));
        assert!(asm.contains("mov rax, 42"));
    }

    #[test]
    fn backend_respects_graph_dependencies_not_text_order() {
        let graph = parser::parse(
            "g0 0.1\nadd %sum %a %b\nconst %a i64 7\nconst %b i64 5\nreturn %sum\n",
        )
        .unwrap();

        let asm = super::x86_64::emit(&graph);

        let first_const = asm.find("mov rax, 7").unwrap();
        let second_const = asm.find("mov rax, 5").unwrap();
        let add = asm.find("add rax").unwrap();

        assert!(first_const < add);
        assert!(second_const < add);
    }

    #[test]
    fn checked_arithmetic_traps_on_overflow() {
        let graph = parser::parse(
            "g0 0.1\nadd %sum %a %b\nconst %a i64 9223372036854775807\nconst %b i64 1\nreturn %sum\n",
        )
        .unwrap();

        let asm = super::x86_64::emit(&graph);

        assert!(asm.contains("jo .Loverflow"));
        assert!(asm.contains(".Loverflow:\n    ud2"));
    }
}
