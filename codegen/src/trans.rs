mod operand;
mod place;
mod rvalue;
mod stmt;
mod term;

use crate::FunctionCtx;
use check::tcx::Tcx;
use check::ty::Layout;
use cranelift::codegen::ir::{AbiParam, ExternalName, InstBuilder, Signature};
use cranelift::codegen::isa::TargetIsa;
use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Backend, DataContext, DataId, FuncId, Linkage, Module, ModuleResult};
use std::collections::BTreeMap;

pub fn translate<'tcx>(
    // target: target_lexicon::Triple,
    tcx: &Tcx<'tcx>,
    package: &mir::Package<'tcx>,
) {
    let target = target_lexicon::Triple::host();
    let flags_builder = cranelift::codegen::settings::builder();
    let flags = cranelift::codegen::settings::Flags::new(flags_builder);
    let isa = cranelift::codegen::isa::lookup(target.clone())
        .unwrap()
        .finish(flags.clone());

    let builder = cranelift_object::ObjectBuilder::new(
        isa,
        "test",
        cranelift_module::default_libcall_names(),
    )
    .unwrap();

    let mut module = Module::<cranelift_object::ObjectBackend>::new(builder);
    let mut func_ids = BTreeMap::new();
    let mut data_ids = BTreeMap::new();
    let mut bytes_count = 0;

    for item in package.items.values() {
        declare(&mut module, tcx, item, &mut func_ids, &mut data_ids).unwrap();
    }

    for item in package.items.values() {
        define(
            &mut module,
            tcx,
            package,
            item,
            &func_ids,
            &data_ids,
            &mut bytes_count,
        )
        .unwrap();
    }
}

pub fn declare<'tcx>(
    module: &mut Module<impl Backend>,
    tcx: &Tcx<'tcx>,
    item: &mir::Item<'tcx>,
    func_ids: &mut BTreeMap<mir::Id, (FuncId, Signature, Layout<'tcx>)>,
    data_ids: &mut BTreeMap<mir::Id, (DataId, Layout<'tcx>)>,
) -> ModuleResult<()> {
    match &item.kind {
        mir::ItemKind::Extern(ty) => {
            if let Some((params, ret)) = ty.func() {
                let mut sig = module.make_signature();
                let ret = tcx.layout(ret);

                match crate::pass::pass_mode(&module, ret) {
                    crate::pass::PassMode::NoPass => {}
                    crate::pass::PassMode::ByVal(ty) => sig.returns.push(AbiParam::new(ty)),
                    crate::pass::PassMode::ByPair(a, b) => {
                        sig.returns.push(AbiParam::new(a));
                        sig.returns.push(AbiParam::new(b));
                    }
                    crate::pass::PassMode::ByRef { .. } => sig
                        .params
                        .push(AbiParam::new(module.target_config().pointer_type())),
                }

                for arg in params {
                    match crate::pass::pass_mode(&module, tcx.layout(arg.ty)) {
                        crate::pass::PassMode::NoPass => {}
                        crate::pass::PassMode::ByVal(ty) => sig.params.push(AbiParam::new(ty)),
                        crate::pass::PassMode::ByPair(a, b) => {
                            sig.params.push(AbiParam::new(a));
                            sig.params.push(AbiParam::new(b));
                        }
                        crate::pass::PassMode::ByRef { .. } => sig
                            .params
                            .push(AbiParam::new(module.target_config().pointer_type())),
                    }
                }

                let func = module.declare_function(&*item.name.symbol, Linkage::Import, &sig)?;

                func_ids.insert(item.id, (func, sig, ret));
            } else {
                let layout = tcx.layout(ty);
                let data = module.declare_data(
                    &*item.name.symbol,
                    Linkage::Import,
                    false,
                    false,
                    Some(layout.align.bytes() as u8),
                )?;

                data_ids.insert(item.id, (data, layout));
            }
        }
        mir::ItemKind::Body(body) => {
            let mut sig = module.make_signature();
            let ret = &body.locals[&mir::LocalId::RET];
            let ret = tcx.layout(ret.ty);

            match crate::pass::pass_mode(&module, ret) {
                crate::pass::PassMode::NoPass => {}
                crate::pass::PassMode::ByVal(ty) => sig.returns.push(AbiParam::new(ty)),
                crate::pass::PassMode::ByPair(a, b) => {
                    sig.returns.push(AbiParam::new(a));
                    sig.returns.push(AbiParam::new(b));
                }
                crate::pass::PassMode::ByRef { .. } => sig
                    .params
                    .push(AbiParam::new(module.target_config().pointer_type())),
            }

            for arg in body.params() {
                match crate::pass::pass_mode(&module, tcx.layout(arg.ty)) {
                    crate::pass::PassMode::NoPass => {}
                    crate::pass::PassMode::ByVal(ty) => sig.params.push(AbiParam::new(ty)),
                    crate::pass::PassMode::ByPair(a, b) => {
                        sig.params.push(AbiParam::new(a));
                        sig.params.push(AbiParam::new(b));
                    }
                    crate::pass::PassMode::ByRef { .. } => sig
                        .params
                        .push(AbiParam::new(module.target_config().pointer_type())),
                }
            }

            let func = module.declare_function(&*item.name.symbol, Linkage::Local, &sig)?;

            func_ids.insert(item.id, (func, sig, ret));
        }
    }

    Ok(())
}

pub fn define<'tcx>(
    module: &mut Module<impl Backend>,
    tcx: &Tcx<'tcx>,
    package: &mir::Package<'tcx>,
    item: &mir::Item<'tcx>,
    func_ids: &BTreeMap<mir::Id, (FuncId, Signature, Layout<'tcx>)>,
    data_ids: &BTreeMap<mir::Id, (DataId, Layout<'tcx>)>,
    bytes_count: &mut usize,
) -> ModuleResult<()> {
    if let mir::ItemKind::Body(body) = &item.kind {
        let (func, sig, _) = &func_ids[&item.id];
        let mut ctx = module.make_context();
        let mut func_ctx = FunctionBuilderContext::new();

        ctx.func.signature = sig.clone();
        ctx.func.name = ExternalName::user(0, item.id.as_u32());

        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let start_block = builder.create_block();

        builder.switch_to_block(start_block);

        let blocks = body
            .blocks
            .keys()
            .map(|id| (*id, builder.create_block()))
            .collect();

        let mut fx = FunctionCtx {
            pointer_type: module.target_config().pointer_type(),
            package,
            tcx,
            module,
            builder,
            body,
            func_ids,
            data_ids,
            blocks,
            locals: BTreeMap::new(),
            bytes_count,
        };

        {
            let ret = body.locals[&mir::LocalId::RET].ty;
            let layout = tcx.layout(ret);

            match crate::pass::pass_mode(fx.module, layout) {
                crate::pass::PassMode::NoPass => {
                    fx.locals
                        .insert(mir::LocalId::RET, crate::place::Place::no_place(layout));
                }
                crate::pass::PassMode::ByVal(_) | crate::pass::PassMode::ByPair(_, _) => {
                    let ssa = crate::analyze::analyze(&fx, mir::LocalId::RET)
                        == crate::analyze::SsaKind::Ssa;

                    local_place(&mut fx, mir::LocalId::RET, layout, ssa);
                }
                crate::pass::PassMode::ByRef { .. } => {
                    let ret_param = fx.builder.append_block_param(start_block, fx.pointer_type);

                    fx.locals.insert(
                        mir::LocalId::RET,
                        crate::place::Place::new_ref(crate::ptr::Pointer::addr(ret_param), layout),
                    );
                }
            }
        }

        let values = body
            .params()
            .map(|param| {
                let layout = tcx.layout(param.ty);

                (
                    crate::pass::value_for_param(&mut fx, start_block, layout),
                    layout,
                )
            })
            .collect::<Vec<_>>();

        for (param, (value, layout)) in body.params().zip(values) {
            let ssa = crate::analyze::analyze(&fx, param.id) == crate::analyze::SsaKind::Ssa;
            let place = local_place(&mut fx, param.id, layout, ssa);

            if let Some(value) = value {
                place.store(&mut fx, value);
            }
        }

        for local in body.locals.values() {
            match &local.kind {
                mir::LocalKind::Var | mir::LocalKind::Tmp => {
                    let ssa =
                        crate::analyze::analyze(&fx, local.id) == crate::analyze::SsaKind::Ssa;
                    let layout = tcx.layout(local.ty);

                    local_place(&mut fx, local.id, layout, ssa);
                }
                _ => {}
            }
        }

        let first_block = *fx.blocks.values().next().unwrap();

        fx.builder.ins().jump(first_block, &[]);

        for (id, block) in &body.blocks {
            fx.builder.switch_to_block(fx.blocks[id]);

            for stmt in &block.stmts {
                fx.trans_stmt(stmt);
            }

            fx.trans_term(&block.term);
        }

        fx.builder.seal_all_blocks();
        fx.builder.finalize();

        println!("{}", fx.builder.func);

        module.define_function(
            *func,
            &mut ctx,
            &mut cranelift::codegen::binemit::NullTrapSink {},
        )?;

        module.clear_context(&mut ctx);
    }

    Ok(())
}

fn local_place<'a, 'tcx>(
    fx: &mut FunctionCtx<'a, 'tcx, impl Backend>,
    id: mir::LocalId,
    layout: Layout<'tcx>,
    ssa: bool,
) -> crate::place::Place<'tcx> {
    let place = if ssa {
        crate::place::Place::new_var(fx, id, layout)
    } else {
        crate::place::Place::new_stack(fx, layout)
    };

    fx.locals.insert(id, place);
    place
}