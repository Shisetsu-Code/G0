//! G0 source-byte emission in bounded chunks, preserving the flat .byte protocol.
use super::*;

pub(super) fn graphs() -> Vec<Graph> {
    let texts = SemanticType::Slice(Box::new(SemanticType::Text));
    let state = vec![SemanticType::Bytes, int(), int(), texts.clone()];
    let mut condition = G::new(
        "emit-program-byte-chunks-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let more = condition.compare(Operation::Lt, input(1), input(2));

    let first = G::new(
        "emit-program-byte-limit-first",
        vec![int(), int()],
        vec![int()],
    );
    let second = G::new(
        "emit-program-byte-limit-second",
        vec![int(), int()],
        vec![int()],
    );
    let mut body = G::new(
        "emit-program-byte-chunks-body",
        state.clone(),
        state.clone(),
    );
    let remaining = body.arithmetic(Operation::Sub, input(2), input(1));
    let bound = body.n(65536);
    let long = body.compare(Operation::Gt, remaining.clone(), bound.clone());
    let count = body.pick("emit-program-byte-limit", long, remaining, bound);
    let chunk = body.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (input(1), int()),
            (count.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let values = body.op(
        Operation::Map {
            body: "emit-byte".into(),
        },
        vec![(chunk, SemanticType::Bytes)],
        texts.clone(),
    );
    let separator = body.text("\n.byte ");
    let text = body.op(
        Operation::TextJoin,
        vec![(values, texts.clone()), (separator, SemanticType::Text)],
        SemanticType::Text,
    );
    let one = body.op(
        Operation::MakeArray,
        vec![(text, SemanticType::Text)],
        texts.clone(),
    );
    let accumulated = body.op(
        Operation::ArrayConcat,
        vec![(input(3), texts.clone()), (one, texts.clone())],
        texts.clone(),
    );
    let next = body.arithmetic(Operation::Add, input(1), count);

    let mut main = G::new(
        "emit-program-byte-values",
        vec![SemanticType::Bytes],
        vec![SemanticType::Text],
    );
    let length = main.length(input(0));
    let length_type = main.ty(&length);
    let length = main.op(
        Operation::ConvertChecked,
        vec![(length, length_type)],
        int(),
    );
    let zero = main.n(0);
    let empty = main.op(Operation::MakeArray, vec![], texts.clone());
    let result = main.loop_node(
        "emit-program-byte-chunks",
        state,
        vec![input(0), zero, length, empty],
    );
    let separator = main.text("\n.byte ");
    let text = main.op(
        Operation::TextJoin,
        vec![(result[3].clone(), texts), (separator, SemanticType::Text)],
        SemanticType::Text,
    );
    vec![
        condition.finish(vec![more]),
        first.finish(vec![input(0)]),
        second.finish(vec![input(1)]),
        body.finish(vec![input(0), next, input(2), accumulated]),
        main.finish(vec![text]),
    ]
}
