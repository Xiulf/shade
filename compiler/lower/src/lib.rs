#![feature(label_break_value)]

use hir::ir as hir;
use lowlang::ir;
use std::collections::HashMap;
use std::sync::Arc;

#[salsa::query_group(LowerDatabaseStorage)]
pub trait LowerDatabase: check::TypeDatabase {
    fn lower(&self, lib: hir::LibId, module: hir::ModuleId) -> Arc<ir::Module>;
}

pub fn lower(db: &dyn LowerDatabase, lib: hir::LibId, module: hir::ModuleId) -> Arc<ir::Module> {
    let file = db.module_tree(lib).file(module);
    let hir = db.module_hir(file);
    let mut converter = Converter::new(db);

    converter.convert(&hir);

    let mut low = converter.finish();

    lowlang::analysis::mandatory(&mut low, &db.target(lib));

    Arc::new(low)
}

pub struct Converter<'db> {
    db: &'db dyn LowerDatabase,
    decls: ir::Decls,
    impls: ir::Impls,
    bodies: ir::Bodies,
}

pub struct BodyConverter<'db, 'c> {
    db: &'db dyn LowerDatabase,
    hir: &'db hir::Module,
    types: Arc<check::TypeCheckResult>,
    builder: ir::Builder<'c>,
    decls: &'c HashMap<hir::DefId, ir::DeclId>,
    locals: HashMap<hir::HirId, ir::Local>,
}

impl<'db> Converter<'db> {
    pub fn new(db: &'db dyn LowerDatabase) -> Self {
        Converter {
            db,
            decls: ir::Decls::new(),
            impls: ir::Impls::new(),
            bodies: ir::Bodies::new(),
        }
    }

    pub fn finish(self) -> ir::Module {
        ir::Module {
            decls: self.decls,
            impls: self.impls,
            bodies: self.bodies,
        }
    }

    pub fn convert(&mut self, hir: &hir::Module) {
        let mut decls = HashMap::with_capacity(hir.imports.len() + hir.items.len());

        for &id in &hir.imports {
            let ty = self.db.typecheck(id);
            let declid = self.decls.next_idx();
            let file = self.db.module_tree(id.lib).file(id.module);
            let hir = self.db.module_hir(file);
            let def = hir.def(id);

            if let Some(link_name) = self.link_name(id) {
                decls.insert(id, declid);
                self.decls.insert(
                    declid,
                    ir::Decl {
                        id: declid,
                        name: link_name,
                        ty: lower_type(self.db, &ty.ty),
                        linkage: ir::Linkage::Import,
                        attrs: if let hir::Def::Item(item) = def {
                            ir::Attrs {
                                c_abi: item.abi() == Some("C"),
                            }
                        } else {
                            ir::Attrs::default()
                        },
                    },
                );
            }
        }

        for (_, item) in &hir.items {
            match &item.kind {
                hir::ItemKind::Func { .. } => {
                    let ty = self.db.typecheck(item.id.owner);
                    let declid = self.decls.next_idx();

                    decls.insert(item.id.owner, declid);
                    self.decls.insert(
                        declid,
                        ir::Decl {
                            id: declid,
                            name: self.link_name(item.id.owner).unwrap(),
                            ty: lower_type(self.db, &ty.ty),
                            linkage: ir::Linkage::Export,
                            attrs: ir::Attrs {
                                c_abi: item.abi() == Some("C"),
                            },
                        },
                    );
                }
                hir::ItemKind::Static { .. } => {
                    let ty = self.db.typecheck(item.id.owner);
                    let declid = self.decls.next_idx();

                    decls.insert(item.id.owner, declid);
                    self.decls.insert(
                        declid,
                        ir::Decl {
                            id: declid,
                            name: self.link_name(item.id.owner).unwrap(),
                            ty: lower_type(self.db, &ty.ty),
                            linkage: ir::Linkage::Export,
                            attrs: ir::Attrs::default(),
                        },
                    );
                }
                hir::ItemKind::Const { .. } => unimplemented!(),
                hir::ItemKind::Foreign { .. } => {
                    if !item.is_intrinsic() {
                        let ty = self.db.typecheck(item.id.owner);
                        let declid = self.decls.next_idx();

                        decls.insert(item.id.owner, declid);
                        self.decls.insert(
                            declid,
                            ir::Decl {
                                id: declid,
                                name: item.name.to_string(),
                                ty: lower_type(self.db, &ty.ty),
                                linkage: ir::Linkage::Export,
                                attrs: ir::Attrs {
                                    c_abi: item.abi() == Some("C"),
                                },
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        for (_, item) in &hir.items {
            match &item.kind {
                hir::ItemKind::Func { body, .. } => {
                    let body = &hir.bodies[body];
                    let ty = self.db.typecheck(item.id.owner);
                    let declid = decls[&item.id.owner];
                    let bodyid = self.bodies.next_idx();
                    let mut b = ir::Body::new(bodyid, declid);
                    let builder = ir::Builder::new(&mut b);
                    let conv = BodyConverter::new(self.db, hir, ty, builder, &decls);

                    conv.convert(body);
                    self.bodies.insert(bodyid, b);
                }
                hir::ItemKind::Static { .. } => unimplemented!(),
                hir::ItemKind::Const { .. } => unimplemented!(),
                _ => {}
            }
        }
    }

    fn link_name(&self, id: hir::DefId) -> Option<String> {
        let file = self.db.module_tree(id.lib).file(id.module);
        let hir = self.db.module_hir(file);
        let def = hir.def(id);

        let name = match def {
            hir::Def::Item(item) => {
                if item.is_intrinsic() {
                    return None;
                } else if item.is_main() {
                    return Some(String::from("main"));
                } else if item.is_no_mangle() {
                    return Some(item.name.to_string());
                } else if let hir::ItemKind::Foreign { .. } = item.kind {
                    return Some(item.name.to_string());
                } else {
                    format!("{}.{}", hir.name, item.name)
                }
            }
            hir::Def::TraitItem(item) => format!("{}.{}", hir.name, item.name),
            hir::Def::ImplItem(item) => format!("{}.{}", hir.name, item.name),
        };

        Some(mangling::mangle(name.bytes()))
    }
}

impl<'db, 'c> BodyConverter<'db, 'c> {
    pub fn new(
        db: &'db dyn LowerDatabase,
        hir: &'db hir::Module,
        types: Arc<check::TypeCheckResult>,
        builder: ir::Builder<'c>,
        decls: &'c HashMap<hir::DefId, ir::DeclId>,
    ) -> Self {
        BodyConverter {
            db,
            hir,
            types,
            builder,
            decls,
            locals: HashMap::new(),
        }
    }

    pub fn convert(mut self, body: &hir::Body) {
        let ret = self.create_header(&body.params);
        let entry = self.builder.create_block();
        let _ = self.builder.set_block(entry);
        let res = self.convert_expr(&body.value);

        self.builder.use_op(ir::Place::new(ret), res);
        self.builder.return_();
    }

    fn create_header(&mut self, params: &[hir::Param]) -> ir::Local {
        use check::ty::Type;
        let mut ty = &self.types.ty;

        if let Type::ForAll(_, ty2) = &**ty {
            ty = ty2;
        }

        if let Type::Func(param_tys, ret) = &**ty {
            let ret = self.builder.create_ret(lower_type(self.db, ret));

            for (param, ty) in params.iter().zip(param_tys) {
                let local = self.builder.create_arg(lower_type(self.db, &ty));

                self.locals.insert(param.id, local);
            }

            ret
        } else {
            self.builder.create_ret(lower_type(self.db, ty))
        }
    }

    fn convert_expr(&mut self, expr: &hir::Expr) -> ir::Operand {
        let ty = lower_type(self.db, &self.types.tys[&expr.id]);

        match &expr.kind {
            hir::ExprKind::Error => unreachable!(),
            hir::ExprKind::Hole { .. } => ir::Operand::Const(ir::Const::Undefined(ty)),
            hir::ExprKind::Ident { res } => match res {
                hir::Res::Error => unreachable!(),
                hir::Res::Def(d, id) => match d {
                    hir::DefKind::Func | hir::DefKind::Static => {
                        ir::Operand::Const(ir::Const::Addr(self.decls[id]))
                    }
                    _ => unreachable!(),
                },
                hir::Res::Local(id) => ir::Operand::Place(ir::Place::new(self.locals[id].clone())),
            },
            hir::ExprKind::Int { val } => ir::Operand::Const(ir::Const::Scalar(*val, ty)),
            hir::ExprKind::Float { bits } => {
                ir::Operand::Const(ir::Const::Scalar(*bits as u128, ty))
            }
            hir::ExprKind::Char { val } => ir::Operand::Const(ir::Const::Scalar(*val as u128, ty)),
            hir::ExprKind::Str { val: _ } => unimplemented!(),
            hir::ExprKind::App { base, args } => self.convert_app(base, args, ty),
            hir::ExprKind::Tuple { exprs } => {
                if exprs.is_empty() {
                    ir::Operand::Const(ir::Const::Tuple(Vec::new()))
                } else {
                    let res = self.builder.create_tmp(ty);
                    let res = ir::Place::new(res);

                    for (i, expr) in exprs.iter().enumerate() {
                        let op = self.convert_expr(expr);

                        self.builder.use_op(res.clone().field(i), op);
                    }

                    ir::Operand::Place(res)
                }
            }
            hir::ExprKind::Record { fields } => {
                if fields.is_empty() {
                    ir::Operand::Const(ir::Const::Tuple(Vec::new()))
                } else {
                    let res = self.builder.create_tmp(ty);
                    let res = ir::Place::new(res);

                    for (i, field) in fields.iter().enumerate() {
                        let op = self.convert_expr(&field.val);

                        self.builder.use_op(res.clone().field(i), op);
                    }

                    ir::Operand::Place(res)
                }
            }
            hir::ExprKind::Field { base, field } => {
                let base_ty = self.types.tys[&base.id].clone();

                if let check::ty::Type::Record(fields, _) = &*base_ty {
                    if let Some(i) = fields.iter().position(|f| f.name == field.symbol) {
                        let op = self.convert_expr(base);
                        let op = self.builder.placed(op, lower_type(self.db, &base_ty));

                        ir::Operand::Place(op.field(i))
                    } else {
                        unreachable!();
                    }
                } else {
                    unreachable!();
                }
            }
            hir::ExprKind::If { cond, then, else_ } => {
                let res = self.builder.create_tmp(ty);
                let res = ir::Place::new(res);
                let cond = self.convert_expr(cond);
                let then_block = self.builder.create_block();
                let else_block = self.builder.create_block();
                let exit_block = self.builder.create_block();

                self.builder
                    .switch(cond, vec![0], vec![else_block, then_block]);
                self.builder.set_block(then_block);

                let then = self.convert_expr(then);

                self.builder.use_op(res.clone(), then);
                self.builder.jump(exit_block);
                self.builder.set_block(else_block);

                let else_ = self.convert_expr(else_);

                self.builder.use_op(res.clone(), else_);
                self.builder.jump(exit_block);
                self.builder.set_block(exit_block);

                ir::Operand::Place(res)
            }
            hir::ExprKind::Case { pred, arms } => {
                let preds = pred
                    .iter()
                    .map(|e| {
                        let op = self.convert_expr(e);
                        let e_ty = &self.types.tys[&e.id];

                        self.builder.placed(op, lower_type(self.db, e_ty))
                    })
                    .collect();

                self.convert_arms(preds, arms, ty)
            }
            hir::ExprKind::Do { block } => self.convert_block(block, ty),
            hir::ExprKind::Typed { expr, .. } => self.convert_expr(expr),
            _ => unimplemented!(),
        }
    }

    fn convert_block(&mut self, block: &hir::Block, ty: ir::Type) -> ir::Operand {
        for (i, stmt) in block.stmts.iter().enumerate() {
            match &stmt.kind {
                hir::StmtKind::Bind { binding } => {
                    let op = self.convert_expr(&binding.val);
                    let bind_ty = &self.types.tys[&binding.val.id];
                    let op = self.builder.placed(op, lower_type(self.db, bind_ty));
                    let block = self.builder.get_block();

                    self.convert_pat(op, &binding.pat, block, block);
                }
                hir::StmtKind::Discard { expr } => {
                    let op = self.convert_expr(expr);

                    if i == block.stmts.len() - 1 {
                        let res = self.builder.create_tmp(ty);
                        let res = ir::Place::new(res);

                        self.builder.use_op(res.clone(), op);

                        return ir::Operand::Place(res);
                    }
                }
            }
        }

        ir::Operand::Const(ir::Const::Undefined(ty))
    }

    fn convert_arms(
        &mut self,
        preds: Vec<ir::Place>,
        arms: &[hir::CaseArm],
        ty: ir::Type,
    ) -> ir::Operand {
        let res = self.builder.create_tmp(ty);
        let res = ir::Place::new(res);
        let exit_block = self.builder.create_block();

        for arm in arms {
            let mut next = self.builder.create_block();

            for (pred, pat) in preds.iter().zip(&arm.pats) {
                next = self.convert_pat(pred.clone(), pat, next, exit_block);
            }

            self.builder.jump(next);
            self.builder.set_block(next);
            self.convert_guarded(&arm.val, res.clone(), exit_block);
            self.builder.jump(exit_block);
        }

        self.builder.jump(exit_block);
        self.builder.set_block(exit_block);

        ir::Operand::Place(res)
    }

    fn convert_guarded(&mut self, guarded: &hir::Guarded, res: ir::Place, _exit_block: ir::Block) {
        match guarded {
            hir::Guarded::Unconditional(expr) => {
                let val = self.convert_expr(expr);

                self.builder.use_op(res, val);
            }
            hir::Guarded::Guarded(_) => unimplemented!(),
        }
    }

    fn convert_pat(
        &mut self,
        pred: ir::Place,
        pat: &hir::Pat,
        next_block: ir::Block,
        exit_block: ir::Block,
    ) -> ir::Block {
        let ty = lower_type(self.db, &self.types.tys[&pat.id]);

        match &pat.kind {
            hir::PatKind::Error => unreachable!(),
            hir::PatKind::Wildcard => next_block,
            hir::PatKind::Bind { sub: None, .. } => {
                if pred.elems.is_empty() && self.builder.local_ty(pred.local) == ty {
                    self.locals.insert(pat.id, pred.local);
                    next_block
                } else {
                    let local = self.builder.create_var(ty);

                    self.locals.insert(pat.id, local);
                    self.builder
                        .use_op(ir::Place::new(local), ir::Operand::Place(pred));

                    next_block
                }
            }
            hir::PatKind::Ctor { ctor: _, pats } => {
                // @todo: implement this properly
                for (i, pat) in pats.iter().enumerate() {
                    self.convert_pat(pred.clone().field(i), pat, next_block, exit_block);
                }

                next_block
            }
            hir::PatKind::Record { fields } => {
                for (i, field) in fields.iter().enumerate() {
                    self.convert_pat(pred.clone().field(i), &field.val, next_block, exit_block);
                }

                next_block
            }
            _ => next_block,
        }
    }

    fn convert_app(&mut self, base: &hir::Expr, args: &[hir::Expr], ty: ir::Type) -> ir::Operand {
        match &base.kind {
            hir::ExprKind::Ident {
                res: hir::Res::Def(hir::DefKind::Ctor, _id),
            } => {
                unimplemented!()
            }
            _ => {
                if let hir::ExprKind::Ident {
                    res: hir::Res::Def(hir::DefKind::Func, id),
                } = &base.kind
                {
                    let file = self.db.module_tree(id.lib).file(id.module);
                    let hir = self.db.module_hir(file);
                    let item_id = hir::HirId {
                        owner: *id,
                        local_id: hir::LocalId(0),
                    };

                    if hir.items[&item_id].is_intrinsic() {
                        let item = &hir.items[&item_id];
                        let mut args = args.iter().map(|a| {
                            (
                                self.convert_expr(a),
                                lower_type(self.db, &self.types.tys[&a.id]),
                            )
                        });

                        return match &**item.name.symbol {
                            "unsafe_read" => {
                                let (arg, arg_ty) = args.next().unwrap();
                                let place = self.builder.placed(arg, arg_ty);

                                ir::Operand::Place(place.deref())
                            }
                            "unsafe_store" => {
                                let (ptr, ptr_ty) = args.next().unwrap();
                                let val = args.next().unwrap().0;
                                let place = self.builder.placed(ptr, ptr_ty);

                                self.builder.use_op(place.deref(), val);

                                ir::Operand::Const(ir::Const::Tuple(Vec::new()))
                            }
                            _ => {
                                let args = args.map(|(a, _)| a).collect();
                                let res = self.builder.create_tmp(ty);
                                let res = ir::Place::new(res);

                                self.builder
                                    .intrinsic(res.clone(), item.name.to_string(), args);

                                ir::Operand::Place(res)
                            }
                        };
                    }
                }

                let res = self.builder.create_tmp(ty.clone());
                let res = ir::Place::new(res);
                let base = self.convert_expr(base);
                let args = args.iter().map(|a| self.convert_expr(a)).collect();

                self.builder.call(vec![res.clone()], base, args);

                ir::Operand::Place(res)
            }
        }
    }
}

fn lower_type(db: &dyn LowerDatabase, ty: &check::ty::Ty) -> ir::Type {
    use check::ty::Type;

    match &**ty {
        Type::Error => unreachable!(),
        Type::Int(_) => unreachable!(),
        Type::Infer(_) => unreachable!(),
        Type::Var(var) => ir::Type::Opaque(var.to_string()),
        Type::TypeOf(id) => lower_type(db, &db.typecheck(*id).ty),
        Type::ForAll(_, ty) => lower_type(db, ty),
        Type::Func(args, ret) => ir::Type::Func(ir::Signature {
            params: args.iter().map(|a| lower_type(db, a)).collect(),
            rets: vec![lower_type(db, ret)],
        }),
        Type::Tuple(tys) => ir::Type::Tuple(tys.iter().map(|t| lower_type(db, t)).collect()),
        Type::Record(fields, None) => {
            ir::Type::Tuple(fields.iter().map(|f| lower_type(db, &f.ty)).collect())
        }
        Type::Record(_fields, Some(_tail)) => unimplemented!(),
        Type::Ctnt(_, ty) => lower_type(db, ty),
        Type::App(base, _, args) => match &**base {
            Type::Data(def) => {
                if *def == db.lang_items().ptr_ty().owner {
                    assert_eq!(args.len(), 1);

                    ir::Type::Ptr(Box::new(lower_type(db, &args[0])))
                } else if *def == db.lang_items().array_ty().owner {
                    unimplemented!();
                } else if *def == db.lang_items().slice_ty().owner {
                    unimplemented!();
                } else if *def == db.lang_items().type_info().owner {
                    ir::Type::Type(args[0].display(db.to_ty_db()).to_string())
                } else if *def == db.lang_items().vwt().owner {
                    ir::Type::Vwt(args[0].display(db.to_ty_db()).to_string())
                } else {
                    lower_type(db, base)
                }
            }
            _ => lower_type(db, base),
        },
        Type::Data(id) => {
            let file = db.module_tree(id.lib).file(id.module);
            let hir = db.module_hir(file);
            let def = hir.def(*id);

            if let hir::Def::Item(item) = def {
                if let Some(repr) = item.repr() {
                    return match repr {
                        "u8" => ir::Type::U8,
                        "u16" => ir::Type::U16,
                        "u32" => ir::Type::U32,
                        "u64" => ir::Type::U64,
                        "u128" => ir::Type::U128,
                        "i8" => ir::Type::I8,
                        "i16" => ir::Type::I16,
                        "i32" => ir::Type::I32,
                        "i64" => ir::Type::I64,
                        "i128" => ir::Type::I128,
                        "f32" => ir::Type::F32,
                        "f64" => ir::Type::F64,
                        _ => unreachable!("unknown repr {}", repr),
                    };
                }
            }

            let variants = db.variants(*id);

            if variants.len() == 1 {
                ir::Type::Tuple(variants[0].tys.iter().map(|t| lower_type(db, t)).collect())
            } else {
                unimplemented!();
            }
        }
    }
}