use crate::*;
use clif::{InstBuilder, Module};
use hir::display::HirDisplay;
use mir::layout::{Abi, Fields, Layout, Variants};
use mir::ty::TypeKind;
use std::sync::Arc;

impl FunctionCtx<'_, '_> {
    pub fn lower(&mut self) {
        // eprintln!("{}", self.body.display(self.db.upcast()));

        for (id, block) in self.body.blocks.iter() {
            self.bcx.switch_to_block(self.blocks[id]);

            for stmt in &block.stmts {
                self.lower_stmt(stmt);
            }

            self.lower_term(&block.term);
        }
    }

    pub fn lower_stmt(&mut self, stmt: &ir::Stmt) {
        match stmt {
            | ir::Stmt::Assign(place, rvalue) => {
                let place = self.lower_place(place);

                self.lower_rvalue(place, rvalue);
            },
            | ir::Stmt::SetDiscr(place, discr) => {
                let place = self.lower_place(place);

                self.lower_set_discr(place, *discr);
            },
            | ir::Stmt::Call(ret, func, args) => {
                let ret = self.lower_place(ret);
                let args = args.iter().map(|a| self.lower_op(a, None)).collect();

                self.lower_call(ret, func, args);
            },
        }
    }

    pub fn lower_term(&mut self, term: &ir::Term) {
        match term {
            | ir::Term::Abort => {
                self.bcx.ins().trap(clif::TrapCode::User(0));
            },
            | ir::Term::Return => {
                let rets = self
                    .body
                    .ret
                    .into_iter()
                    .flat_map(|r| self.value_for_ret(r))
                    .collect::<Vec<_>>();

                self.bcx.ins().return_(&rets);
            },
            | ir::Term::Jump(to) => {
                self.bcx.ins().jump(self.blocks[*to], &[]);
            },
            | ir::Term::Switch(op, vals, blocks) => {
                let mut switch = clif::Switch::new();
                let otherwise = self.blocks[*blocks.last().unwrap()];
                let val = self.lower_op(op, None);
                let val = val.load_scalar(self);

                for (val, block) in vals.iter().zip(blocks) {
                    switch.set_entry(*val, self.blocks[*block]);
                }

                switch.emit(&mut self.bcx, val, otherwise);
            },
        }
    }

    pub fn lower_rvalue(&mut self, place: PlaceRef, rvalue: &ir::RValue) {
        match rvalue {
            | ir::RValue::Use(op) => {
                self.lower_op(op, Some(place));
            },
            | ir::RValue::AddrOf(val) => {
                let val = self.lower_place(val);

                val.write_place_ref(self, place);
            },
            | ir::RValue::GetDiscr(val) => {
                let val = self.lower_place(val);
                let val = val.to_value(self);

                self.lower_get_discr(place, val);
            },
            | ir::RValue::Intrinsic(name, args) => {
                let args = args.iter().map(|a| self.lower_op(a, None)).collect();

                self.lower_intrinsic(place, name, args);
            },
        }
    }

    pub fn lower_set_discr(&mut self, place: PlaceRef, discr: u128) {
        match place.layout.variants.clone() {
            | Variants::Single { index } => {
                assert_eq!(index, discr as usize);
            },
            | Variants::Multiple {
                tag: _,
                tag_field,
                tag_encoding: mir::layout::TagEncoding::Direct,
                variants: _,
            } => {
                let ptr = place.field(self, tag_field);
                let discr = ValueRef::new_const(discr, self, ptr.layout.clone());

                ptr.store(self, discr);
            },
            | Variants::Multiple {
                tag: _,
                tag_field,
                tag_encoding:
                    mir::layout::TagEncoding::Niche {
                        dataful_variant,
                        niche_variants,
                        niche_start,
                    },
                variants: _,
            } => {
                if discr != dataful_variant as u128 {
                    let niche = place.field(self, tag_field);
                    let niche_value = discr - *niche_variants.start() as u128;
                    let niche_value = niche_value.wrapping_add(niche_start);
                    let niche_val = ValueRef::new_const(niche_value, self, niche.layout.clone());

                    niche.store(self, niche_val);
                }
            },
        }
    }

    pub fn lower_get_discr(&mut self, place: PlaceRef, discr: ValueRef) {
        let (_tag_scalar, tag_field, tag_encoding) = match &discr.layout.variants {
            | Variants::Single { index } => {
                let val = ValueRef::new_const(*index as u128, self, place.layout.clone());

                place.store(self, val);
                return;
            },
            | Variants::Multiple {
                tag,
                tag_field,
                tag_encoding,
                variants: _,
            } => (tag.clone(), *tag_field, tag_encoding.clone()),
        };

        let tag = discr.field(self, tag_field);

        match tag_encoding {
            | mir::layout::TagEncoding::Direct => {
                place.store(self, tag);
            },
            | mir::layout::TagEncoding::Niche { .. } => unimplemented!(),
        }
    }

    pub fn lower_call(&mut self, ret: PlaceRef, func: &ir::Operand, args: Vec<ValueRef>) {
        let ret_mode = self.pass_mode(&ret.layout);
        let ret_ptr = match ret_mode {
            | abi::PassMode::ByRef { size: _ } => Some(ret.as_ptr().get_addr(self)),
            | _ => None,
        };

        let arg_layouts = args.iter().map(|a| a.layout.clone()).collect::<Vec<_>>();
        let mut args = ret_ptr
            .into_iter()
            .chain(args.into_iter().flat_map(|a| self.value_for_arg(a)))
            .collect::<Vec<_>>();

        let inst = if let ir::Operand::Const(ir::Const::Addr(id), _) = func {
            let func = self.func_id(id);
            let func = self.mcx.module.declare_func_in_func(func, &mut self.bcx.func);

            self.bcx.ins().call(func, &args)
        } else {
            let func_ty = self.body.operand_type(func);
            let mut sig = self.mk_signature(&ret.layout, &arg_layouts);
            let func = self.lower_op(func, None);

            if let TypeKind::Clos(_, _) = func_ty.kind {
                let (env, func) = func.load_scalar_pair(self);
                let ptr_type = self.module.target_config().pointer_type();

                sig.params.insert(0, clif::AbiParam::new(ptr_type));
                args.insert(0, env);

                let sig = self.bcx.import_signature(sig);

                self.bcx.ins().call_indirect(sig, func, &args)
            } else {
                let func = func.load_scalar(self);
                let sig = self.bcx.import_signature(sig);

                self.bcx.ins().call_indirect(sig, func, &args)
            }
        };

        let mut res = self
            .bcx
            .inst_results(inst)
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .into_iter();

        match ret_mode {
            | abi::PassMode::NoPass => {},
            | abi::PassMode::ByRef { size: _ } => {},
            | abi::PassMode::ByVal(_) => {
                let ret_val = res.next().unwrap();
                let ret_val = ValueRef::new_val(ret_val, ret.layout.clone());

                ret.store(self, ret_val);
            },
            | abi::PassMode::ByValPair(_, _) => {
                let val1 = res.next().unwrap();
                let val2 = res.next().unwrap();
                let ret_val = ValueRef::new_val_pair(val1, val2, ret.layout.clone());

                ret.store(self, ret_val);
            },
        }
    }

    pub fn lower_place(&mut self, place: &ir::Place) -> place::PlaceRef {
        let mut res = self.locals[place.local].clone();

        for elem in &place.elems {
            match elem {
                | ir::PlaceElem::Deref => res = res.deref(self),
                | ir::PlaceElem::Field(field) => res = res.field(self, *field),
                | ir::PlaceElem::Index(op) => {
                    let idx = self.lower_op(op, None);

                    res = res.index(self, idx);
                },
                | ir::PlaceElem::Offset(op) => {
                    let offset = self.lower_op(op, None);

                    res = res.offset(self, offset);
                },
                | ir::PlaceElem::Downcast(idx) => res = res.downcast_variant(self, *idx),
            }
        }

        res
    }

    fn lower_op(&mut self, op: &ir::Operand, into: Option<place::PlaceRef>) -> value::ValueRef {
        match op {
            | ir::Operand::Record(_, _) => unimplemented!(),
            | ir::Operand::Place(place) => {
                let place = self.lower_place(place);
                let value = place.to_value(self);

                if let Some(into) = into {
                    into.store(self, value.clone());
                }

                value
            },
            | ir::Operand::Const(c, ty) => {
                let layout = self.db.layout_of(ty.clone());

                self.lower_const(c, layout, into)
            },
        }
    }

    fn lower_const(&mut self, c: &ir::Const, layout: Arc<Layout>, into: Option<place::PlaceRef>) -> value::ValueRef {
        if let Some(into) = into {
            match c {
                | ir::Const::Undefined => {},
                | ir::Const::Scalar(s) => {
                    let val = ValueRef::new_const(*s, self, layout);

                    into.clone().store(self, val.clone());
                },
                | ir::Const::Tuple(cs) => {
                    for (i, c) in cs.iter().enumerate() {
                        let lyt = layout.field(self.db.upcast(), i).unwrap();
                        let field = into.clone().field(self, i);

                        self.lower_const(c, lyt, Some(field));
                    }
                },
                | ir::Const::Addr(id) => {
                    let ptr_type = self.module.target_config().pointer_type();
                    let val = if let hir::id::DefWithBodyId::StaticId(id) = id.def {
                        let global = self.static_ids[&id.into()];
                        let global = self.mcx.module.declare_data_in_func(global, &mut self.bcx.func);
                        let global = self.bcx.ins().global_value(ptr_type, global);

                        ValueRef::new_val(global, layout)
                    } else {
                        let func = self.func_ids[id].0;
                        let func = self.mcx.module.declare_func_in_func(func, &mut self.bcx.func);
                        let func = self.bcx.ins().func_addr(ptr_type, func);

                        ValueRef::new_val(func, layout)
                    };

                    into.clone().store(self, val);
                },
                | ir::Const::String(s) => {
                    let data_id = self.alloc_string(s);
                    let ptr_type = self.module.target_config().pointer_type();
                    let global = self.mcx.module.declare_data_in_func(data_id, &mut self.bcx.func);
                    let global = self.bcx.ins().global_value(ptr_type, global);
                    let ptr_field = layout.field(self.db.upcast(), 0).unwrap();
                    let len_field = layout.field(self.db.upcast(), 1).unwrap();
                    let ptr = ValueRef::new_val(global, ptr_field);
                    let len = ValueRef::new_const(s.len() as u128, self, len_field);

                    into.clone().field(self, 0).store(self, ptr);
                    into.clone().field(self, 1).store(self, len);
                },
                | _ => unimplemented!("{}", c.display(self.db.upcast())),
            }

            into.to_value(self)
        } else {
            match c {
                | ir::Const::Undefined => match &layout.abi {
                    | Abi::Scalar(_) => ValueRef::new_const(0, self, layout),
                    | _ if layout.is_zst() => ValueRef::new_zst(layout),
                    | _ => {
                        let slot = self.bcx.create_stack_slot(clif::StackSlotData::new(
                            clif::StackSlotKind::ExplicitSlot,
                            layout.size.bytes() as u32,
                        ));

                        ValueRef::new_ref(Pointer::stack(slot), layout)
                    },
                },
                | ir::Const::Scalar(s) => ValueRef::new_const(*s, self, layout),
                | ir::Const::Tuple(cs) if cs.is_empty() => ValueRef::new_unit(),
                | ir::Const::Tuple(cs) => match &layout.abi {
                    | Abi::Uninhabited => unreachable!(),
                    | Abi::Scalar(_) => {
                        assert_eq!(cs.len(), 1);
                        self.lower_const(&cs[0], layout, None)
                    },
                    | _ => {
                        let place = PlaceRef::new_stack(self, layout.clone());

                        for (i, c) in cs.iter().enumerate() {
                            let layout = layout.field(self.db.upcast(), i).unwrap();
                            let place = place.clone().field(self, i);

                            self.lower_const(c, layout, Some(place));
                        }

                        place.to_value(self)
                    },
                },
                | ir::Const::Addr(id) => {
                    let ptr_type = self.module.target_config().pointer_type();

                    if let hir::id::DefWithBodyId::StaticId(id) = id.def {
                        let global = self.static_ids[&id.into()];
                        let global = self.mcx.module.declare_data_in_func(global, &mut self.bcx.func);
                        let global = self.bcx.ins().global_value(ptr_type, global);

                        ValueRef::new_val(global, layout)
                    } else {
                        let func = self.func_ids[id].0;
                        let func = self.mcx.module.declare_func_in_func(func, &mut self.bcx.func);
                        let func = self.bcx.ins().func_addr(ptr_type, func);

                        ValueRef::new_val(func, layout)
                    }
                },
                | ir::Const::String(s) => {
                    let data_id = self.alloc_string(s);
                    let ptr_type = self.module.target_config().pointer_type();
                    let global = self.mcx.module.declare_data_in_func(data_id, &mut self.bcx.func);
                    let global = self.bcx.ins().global_value(ptr_type, global);
                    let len = self.bcx.ins().iconst(ptr_type, s.len() as i64);

                    ValueRef::new_ref_meta(Pointer::addr(global), len, layout)
                },
                | ir::Const::Ref(to) => {
                    let elem = layout.elem(self.db.upcast()).unwrap();
                    let data_id = self.alloc_const(to, elem, None);
                    let ptr_type = self.module.target_config().pointer_type();
                    let global = self.mcx.module.declare_data_in_func(data_id, &mut self.bcx.func);
                    let global = self.bcx.ins().global_value(ptr_type, global);

                    ValueRef::new_val(global, layout)
                },
                | _ => unimplemented!(),
            }
        }
    }

    fn alloc_string(&mut self, s: &str) -> clif::DataId {
        let id = self.module.declare_anonymous_data(false, false).unwrap();
        let mut dcx = clif::DataContext::new();

        dcx.define(s.as_bytes().into());

        self.module.define_data(id, &dcx).unwrap();
        id
    }

    fn alloc_const(&mut self, c: &ir::Const, layout: Arc<Layout>, into: Option<clif::DataId>) -> clif::DataId {
        let data_id = into.unwrap_or_else(|| self.module.declare_anonymous_data(false, false).unwrap());
        let mut dcx = clif::DataContext::new();
        let mut bytes = Vec::with_capacity(layout.size.bytes() as usize);

        bytes.resize(bytes.capacity(), 0);
        rec(self, &mut dcx, c, layout, &mut bytes, 0);
        dcx.define(bytes.into());
        self.module.define_data(data_id, &dcx).unwrap();

        return data_id;

        fn rec(
            fx: &mut FunctionCtx,
            dcx: &mut clif::DataContext,
            c: &ir::Const,
            layout: Arc<Layout>,
            bytes: &mut [u8],
            offset: usize,
        ) {
            match c {
                | ir::Const::Undefined => {},
                | ir::Const::Scalar(s) => match layout.size.bytes() {
                    | 1 => bytes[0] = *s as u8,
                    | 2 => {
                        let ptr = bytes.as_mut_ptr() as *mut u16;

                        unsafe {
                            *ptr = *s as u16;
                        }
                    },
                    | 4 => {
                        let ptr = bytes.as_mut_ptr() as *mut u32;

                        unsafe {
                            *ptr = *s as u32;
                        }
                    },
                    | 8 => {
                        let ptr = bytes.as_mut_ptr() as *mut u64;

                        unsafe {
                            *ptr = *s as u64;
                        }
                    },
                    | 16 => {
                        let ptr = bytes.as_mut_ptr() as *mut u128;

                        unsafe {
                            *ptr = *s;
                        }
                    },
                    | _ => unreachable!(),
                },
                | ir::Const::Tuple(cs) => match &layout.fields {
                    | Fields::Primitive => unimplemented!(),
                    | Fields::Array { stride, count } => {
                        assert_eq!(*count, cs.len());
                        let mut off = 0;
                        let stride = stride.bytes() as usize;

                        for i in 0..*count {
                            let field = layout.field(fx.db.upcast(), i).unwrap();

                            rec(fx, dcx, &cs[i], field, &mut bytes[off..off + stride], offset + off);
                            off += stride;
                        }
                    },
                    | Fields::Union { .. } => unimplemented!(),
                    | Fields::Arbitrary { fields } => {
                        for (i, (off, field)) in fields.iter().enumerate() {
                            let off = off.bytes() as usize;
                            let size = field.size.bytes() as usize;

                            rec(
                                fx,
                                dcx,
                                &cs[i],
                                field.clone(),
                                &mut bytes[off..off + size],
                                offset + off,
                            );
                        }
                    },
                },
                | ir::Const::Ref(to) => {
                    let elem = layout.elem(fx.db.upcast()).unwrap();
                    let data_id = fx.alloc_const(to, elem, None);
                    let global = fx.module.declare_data_in_data(data_id, dcx);

                    dcx.write_data_addr(offset as u32, global, 0);
                },
                | _ => unimplemented!("{:?}", c),
            }
        }
    }

    fn func_id(&mut self, func: &ir::BodyId) -> clif::FuncId {
        if let Some((id, _)) = self.func_ids.get(func) {
            *id
        } else {
            let sig = self.func_signature(*func);
            let mut name = match func.def {
                | hir::id::DefWithBodyId::FuncId(func) => {
                    let func: hir::Func = func.into();

                    func.link_name(self.db.upcast()).to_string()
                },
                | _ => unreachable!(),
            };

            let local_id: u32 = func.local_id.into_raw().into();

            if local_id != 0 {
                name = format!("{}^{}", name, local_id);
            }

            self.mcx
                .module
                .declare_function(&name, clif::Linkage::Import, &sig)
                .unwrap()
        }
    }
}
