pub mod x86_64 {
    use crate::graph::{Graph, Op};

    fn slot(id: usize) -> usize { (id + 1) * 8 }

    pub fn emit(graph: &Graph) -> String {
        let frame = ((graph.nodes.len() * 8 + 15) / 16) * 16;
        let mut out = String::new();
        out.push_str(".intel_syntax noprefix\n.text\n.global g0_main\n.type g0_main, @function\ng0_main:\n");
        out.push_str("    push rbp\n    mov rbp, rsp\n");
        if frame != 0 {
            out.push_str(&format!("    sub rsp, {frame}\n"));
        }

        for (id, node) in graph.nodes.iter().enumerate() {
            let dst = slot(id);
            match node.op {
                Op::Const(value) => {
                    out.push_str(&format!("    mov rax, {value}\n    mov QWORD PTR [rbp-{dst}], rax\n"));
                }
                Op::Add(a, b) => {
                    out.push_str(&format!(
                        "    mov rax, QWORD PTR [rbp-{}]\n    add rax, QWORD PTR [rbp-{}]\n    mov QWORD PTR [rbp-{dst}], rax\n",
                        slot(a), slot(b)
                    ));
                }
                Op::Sub(a, b) => {
                    out.push_str(&format!(
                        "    mov rax, QWORD PTR [rbp-{}]\n    sub rax, QWORD PTR [rbp-{}]\n    mov QWORD PTR [rbp-{dst}], rax\n",
                        slot(a), slot(b)
                    ));
                }
                Op::Mul(a, b) => {
                    out.push_str(&format!(
                        "    mov rax, QWORD PTR [rbp-{}]\n    imul rax, QWORD PTR [rbp-{}]\n    mov QWORD PTR [rbp-{dst}], rax\n",
                        slot(a), slot(b)
                    ));
                }
            }
        }

        out.push_str(&format!("    mov rax, QWORD PTR [rbp-{}]\n    leave\n    ret\n", slot(graph.output)));
        out.push_str(".size g0_main, .-g0_main\n.section .note.GNU-stack,\"\" ,@progbits\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{compile_source, parser, optimize, validate};

    #[test]
    fn emits_x86_64_function() {
        let asm = compile_source("g0 0.1\nconst %a i64 20\nconst %b i64 22\nadd %sum %a %b\nreturn %sum\n").unwrap();
        assert!(asm.contains(".global g0_main"));
        assert!(asm.contains("mov rax, 42"));
    }

    #[test]
    fn backend_keeps_nonconstant_ops() {
        let mut graph = parser::parse("g0 0.1\nconst %a i64 7\nconst %b i64 5\nadd %sum %a %b\nreturn %sum\n").unwrap();
        validate::validate(&graph).unwrap();
        optimize::constant_fold(&mut graph);
        let asm = super::x86_64::emit(&graph);
        assert!(asm.contains("mov rax, 12"));
    }
}
