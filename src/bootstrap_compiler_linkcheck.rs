//! Program-name resolution and control contract checks executed by G0.
use super::*;
fn names_graphs() -> Vec<Graph> {
    let mut equal=G::new("validator-name-equal",vec![SemanticType::Bytes,int(),int()],vec![SemanticType::Bool]);
    let left_size=equal.u32(input(1));let right_size=equal.u32(input(2));
    let sizes=equal.compare(Operation::Eq,left_size.clone(),right_size.clone());
    let smaller=equal.compare(Operation::Lt,left_size.clone(),right_size.clone());
    let count=equal.op(Operation::Select{when_true:"validator-name-min-left".into(),when_false:"validator-name-min-right".into()},vec![(smaller,SemanticType::Bool),(left_size,int()),(right_size,int())],int());
    let node=equal.b.graph.nodes.last_mut().unwrap();node.inputs[0].name="selector".into();node.inputs[1].name="p0".into();node.inputs[2].name="p1".into();
    let min_left=G::new("validator-name-min-left",vec![int(),int()],vec![int()]);
    let min_right=G::new("validator-name-min-right",vec![int(),int()],vec![int()]);
    let left=equal.advance(input(1),4);let right=equal.advance(input(2),4);
    let contents=call(&mut equal,"validator-byte-equal",vec![input(0),left,right,count],SemanticType::Bool);
    let names_equal=equal.and(sizes,contents);

    let state=vec![SemanticType::Bytes,rows(),int(),int(),int()];
    let mut cond=G::new("validator-name-count-condition",state.clone(),vec![SemanticType::Bool]);
    let len=cond.length(input(1));let count_more=cond.compare(Operation::Lt,input(3),len);
    let mut body=G::new("validator-name-count-body",state.clone(),state.clone());
    let descriptor=body.index(input(1),input(3));let at=body.field(descriptor,2);
    let same=call(&mut body,"validator-name-equal",vec![input(0),input(2),at],SemanticType::Bool);
    let one=body.n(1);
    let increment=body.op(Operation::Select{when_true:"validator-name-one".into(),when_false:"validator-name-zero".into()},vec![(same,SemanticType::Bool)],int());
    let count_next=body.arithmetic(Operation::Add,input(3),one);let count_total=body.arithmetic(Operation::Add,input(4),increment);
    let mut main=G::new("validator-name-count",vec![SemanticType::Bytes,rows(),int()],vec![int()]);
    let start=main.n(0);let count_zero=main.n(0);let result=main.loop_node("validator-name-count",state.clone(),vec![input(0),input(1),input(2),start,count_zero]);
    let mut one_graph=G::new("validator-name-one",vec![],vec![int()]);let one_value=one_graph.n(1);
    let mut zero_graph=G::new("validator-name-zero",vec![],vec![int()]);let zero_value=zero_graph.n(0);

    let unique_state=vec![SemanticType::Bytes,rows(),int(),SemanticType::Bool];
    let mut unique_cond=G::new("validator-unique-names-condition",unique_state.clone(),vec![SemanticType::Bool]);
    let len=unique_cond.length(input(1));let more=unique_cond.compare(Operation::Lt,input(2),len);let more=unique_cond.and(more,input(3));
    let mut unique_body=G::new("validator-unique-names-body",unique_state.clone(),unique_state.clone());
    let row=unique_body.index(input(1),input(2));let at=unique_body.field(row,2);
    let count=call(&mut unique_body,"validator-name-count",vec![input(0),input(1),at.clone()],int());
    let one=unique_body.n(1);let unique=unique_body.compare(Operation::Eq,count,one.clone());
    let len=unique_body.u32(at);let zero=unique_body.n(0);let nonempty=unique_body.compare(Operation::Gt,len,zero);
    let valid=unique_body.and(unique,nonempty);let valid=unique_body.and(input(3),valid);
    let unique_next=unique_body.arithmetic(Operation::Add,input(2),one);let unique_valid=valid;
    let mut unique=G::new("validator-program-names",vec![SemanticType::Bytes],vec![SemanticType::Bool]);
    let descriptors=call(&mut unique,"reader-program-ast",vec![input(0)],rows());
    let zero=unique.n(0);let yes=unique.bool(true);let result_unique=unique.loop_node("validator-unique-names",unique_state,vec![input(0),descriptors.clone(),zero,yes]);
    let entry=unique.n(8);let entry_count=call(&mut unique,"validator-name-count",vec![input(0),descriptors,entry],int());
    let one=unique.n(1);let entry_valid=unique.compare(Operation::Eq,entry_count,one);let valid=unique.and(result_unique[3].clone(),entry_valid);
    vec![equal.finish(vec![names_equal]),min_left.finish(vec![input(0)]),min_right.finish(vec![input(1)]),cond.finish(vec![count_more]),body.finish(vec![input(0),input(1),input(2),count_next,count_total]),main.finish(vec![result[4].clone()]),one_graph.finish(vec![one_value]),zero_graph.finish(vec![zero_value]),unique_cond.finish(vec![more]),unique_body.finish(vec![input(0),input(1),unique_next,unique_valid]),unique.finish(vec![valid])]
}
fn reference_graphs()->Vec<Graph>{
    let args=vec![SemanticType::Bytes,rows(),int()];
    let mut target=G::new("validator-reference-target",args.clone(),vec![SemanticType::Bool]);
    let count=call(&mut target,"validator-name-count",vec![input(0),input(1),input(2)],int());
    let one=target.n(1);let target_ok=target.compare(Operation::Eq,count,one);
    let mut single=G::new("validator-reference-single",args.clone(),vec![SemanticType::Bool]);
    let at=single.advance(input(2),1);let single_ok=call(&mut single,"validator-reference-target",vec![input(0),input(1),at],SemanticType::Bool);
    let mut pair=G::new("validator-reference-pair",args.clone(),vec![SemanticType::Bool]);
    let first=pair.advance(input(2),1);let second=pair.skip_blob(first.clone());
    let a=call(&mut pair,"validator-reference-target",vec![input(0),input(1),first],SemanticType::Bool);
    let b=call(&mut pair,"validator-reference-target",vec![input(0),input(1),second],SemanticType::Bool);let pair_ok=pair.and(a,b);
    let mut unchanged=G::new("validator-reference-none",args.clone(),vec![SemanticType::Bool]);let unchanged_yes=unchanged.bool(true);

    let arm_state=vec![SemanticType::Bytes,rows(),int(),int(),SemanticType::Bool];
    let mut arm_cond=G::new("validator-reference-arms-condition",arm_state.clone(),vec![SemanticType::Bool]);
    let zero=arm_cond.n(0);let more=arm_cond.compare(Operation::Gt,input(3),zero);let arm_more=arm_cond.and(more,input(4));
    let mut arm_body=G::new("validator-reference-arms-body",arm_state.clone(),arm_state.clone());
    let target_at=arm_body.skip_blob(input(2));let next=arm_body.skip_blob(target_at.clone());
    let good=call(&mut arm_body,"validator-reference-target",vec![input(0),input(1),target_at],SemanticType::Bool);let arm_valid=arm_body.and(input(4),good);
    let one=arm_body.n(1);let arm_left=arm_body.arithmetic(Operation::Sub,input(3),one);
    let mut matched=G::new("validator-reference-match",args.clone(),vec![SemanticType::Bool]);
    let count_at=matched.advance(input(2),1);let count=matched.u32(count_at.clone());let cursor=matched.advance(count_at,4);let yes=matched.bool(true);
    let result=matched.loop_node("validator-reference-arms",arm_state,vec![input(0),input(1),cursor,count,yes]);
    let otherwise=call(&mut matched,"validator-reference-target",vec![input(0),input(1),result[2].clone()],SemanticType::Bool);let match_ok=matched.and(result[4].clone(),otherwise);
    let mut graphs=vec![target.finish(vec![target_ok]),single.finish(vec![single_ok]),pair.finish(vec![pair_ok]),unchanged.finish(vec![unchanged_yes]),arm_cond.finish(vec![arm_more]),arm_body.finish(vec![input(0),input(1),next,arm_left,arm_valid]),matched.finish(vec![match_ok])];
    let dispatch=[("validator-reference-node",47,"validator-reference-single","validator-reference-map"),("validator-reference-map",7,"validator-reference-single","validator-reference-select"),("validator-reference-select",4,"validator-reference-pair","validator-reference-loop"),("validator-reference-loop",6,"validator-reference-pair","validator-reference-choice"),("validator-reference-choice",5,"validator-reference-match","validator-reference-none")];
    for (name,tag,yes,no) in dispatch {
        let mut g=G::new(name,args.clone(),vec![SemanticType::Bool]);let actual=g.byte(input(2));let expected=g.n(tag);let test=g.compare(Operation::Eq,actual,expected);
        let valid=select(&mut g,yes,no,test,vec![input(0),input(1),input(2)]);graphs.push(g.finish(vec![valid]));
    }
    let node_state=vec![SemanticType::Bytes,rows(),rows(),int(),SemanticType::Bool];
    let mut nc=G::new("validator-reference-nodes-condition",node_state.clone(),vec![SemanticType::Bool]);let len=nc.length(input(2));let more=nc.compare(Operation::Lt,input(3),len);let node_more=nc.and(more,input(4));
    let mut nb=G::new("validator-reference-nodes-body",node_state.clone(),node_state.clone());let row=nb.index(input(2),input(3));let operation=nb.field(row,2);
    let valid=call(&mut nb,"validator-reference-node",vec![input(0),input(1),operation],SemanticType::Bool);let node_valid=nb.and(input(4),valid);let one=nb.n(1);let node_next=nb.arithmetic(Operation::Add,input(3),one);
    let mut graph=G::new("validator-reference-graph",args,vec![SemanticType::Bool]);let nodes=call(&mut graph,"reader-graph-ast",vec![input(0),input(2)],rows());let zero=graph.n(0);let yes=graph.bool(true);let nodes_result=graph.loop_node("validator-reference-nodes",node_state,vec![input(0),input(1),nodes,zero,yes]);
    let program_state=vec![SemanticType::Bytes,rows(),int(),SemanticType::Bool];
    let mut pc=G::new("validator-reference-program-condition",program_state.clone(),vec![SemanticType::Bool]);let len=pc.length(input(1));let more=pc.compare(Operation::Lt,input(2),len);let program_more=pc.and(more,input(3));
    let mut pb=G::new("validator-reference-program-body",program_state.clone(),program_state.clone());let descriptor=pb.index(input(1),input(2));let start=pb.field(descriptor,0);let valid=call(&mut pb,"validator-reference-graph",vec![input(0),input(1),start],SemanticType::Bool);let program_valid=pb.and(input(3),valid);let one=pb.n(1);let program_next=pb.arithmetic(Operation::Add,input(2),one);
    let mut main=G::new("validator-program-references",vec![SemanticType::Bytes],vec![SemanticType::Bool]);let descriptors=call(&mut main,"reader-program-ast",vec![input(0)],rows());let zero=main.n(0);let yes=main.bool(true);let program=main.loop_node("validator-reference-program",program_state,vec![input(0),descriptors,zero,yes]);
    graphs.extend([nc.finish(vec![node_more]),nb.finish(vec![input(0),input(1),input(2),node_next,node_valid]),graph.finish(vec![nodes_result[4].clone()]),pc.finish(vec![program_more]),pb.finish(vec![input(0),input(1),program_next,program_valid]),main.finish(vec![program[3].clone()])]);graphs
}
pub(super) fn graphs()->Vec<Graph>{let mut g=names_graphs();g.extend(reference_graphs());g}
