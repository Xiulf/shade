use crate::ty::*;
use crate::TypeDatabase;
use hir::ir;
use std::fmt::{Display, Formatter, Result, Write};

pub trait TypedDisplay<S = ()> {
    fn typed_fmt(&self, db: &dyn TypeDatabase, s: &S, f: &mut Formatter) -> Result;
}

pub struct Typed<'a, S, T>(pub &'a dyn TypeDatabase, pub &'a S, pub &'a T);

impl<'a, S, T: TypedDisplay<S>> Display for Typed<'a, S, T> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.2.typed_fmt(self.0, self.1, f)
    }
}

impl TypedDisplay for Ty {
    fn typed_fmt(&self, db: &dyn TypeDatabase, _: &(), f: &mut Formatter) -> Result {
        let is_func = |f: &Ty| -> bool {
            if let Type::Ctor(f) = &**f {
                *f == db.lang_items().fn_ty().owner
            } else {
                false
            }
        };

        let is_record = |f: &Ty| -> bool {
            if let Type::Ctor(f) = &**f {
                *f == db.lang_items().record_ty().owner
            } else {
                false
            }
        };

        match &**self {
            Type::Error => write!(f, "{{error}}"),
            Type::Unknown(u) => write!(f, "?{}", u.0),
            Type::Skolem(v, _, _, _) => write!(f, "?{}", v),
            Type::Var(v) => v.fmt(f),
            Type::Int(i) => i.fmt(f),
            Type::String(s) => write!(f, "{:?}", s),
            Type::Tuple(ts) => write!(f, "({})", Typed(db, &(), &ts)),
            Type::Row(fs, None) => write!(f, "({})", Typed(db, &(), &fs)),
            Type::Row(fs, Some(tail)) => {
                write!(f, "({} | {})", Typed(db, &(), &fs), Typed(db, &(), tail))
            }
            Type::ForAll(vars, ty, _) => {
                write!(f, "forall")?;

                for (var, kind) in vars {
                    if let Some(kind) = kind {
                        write!(f, " ({} :: {})", var, Typed(db, &(), &kind))?;
                    } else {
                        write!(f, " {}", var)?;
                    }
                }

                write!(f, ". {}", Typed(db, &(), ty))
            }
            Type::Ctnt(ctnt, ty) => write!(f, "{} => {}", Typed(db, &(), ctnt), Typed(db, &(), ty)),
            Type::Ctor(id) => {
                let file = db.module_tree(id.lib).file(id.module);
                let hir = db.module_hir(file);
                let def = hir.def(*id);

                def.name().fmt(f)
            }
            Type::App(fu, targs) if is_func(fu) && targs.len() == 2 => {
                write!(
                    f,
                    "{} -> {}",
                    Typed(db, &(), &targs[0]),
                    Typed(db, &(), &targs[1])
                )
            }
            Type::App(fu, targs) if is_record(fu) && targs.len() == 1 => {
                if let Type::Row(fs, Some(tail)) = &*targs[0] {
                    write!(
                        f,
                        "{{ {} | {} }}",
                        Typed(db, &(), &fs),
                        Typed(db, &(), tail)
                    )
                } else if let Type::Row(fs, None) = &*targs[0] {
                    write!(f, "{{ {} }}", Typed(db, &(), &fs))
                } else {
                    unreachable!();
                }
            }
            Type::App(base, args) | Type::KindApp(base, args) => {
                if base.needs_parens() {
                    write!(f, "({})", Typed(db, &(), base))?;
                } else {
                    base.typed_fmt(db, &(), f)?;
                }

                for arg in args.iter() {
                    write!(f, " ")?;

                    if arg.needs_parens() {
                        write!(f, "({})", Typed(db, &(), arg))?;
                    } else {
                        arg.typed_fmt(db, &(), f)?;
                    }
                }

                Ok(())
            }
        }
    }
}

impl<T: TypedDisplay> TypedDisplay for &List<T> {
    fn typed_fmt(&self, db: &dyn TypeDatabase, _: &(), f: &mut Formatter) -> Result {
        for (i, t) in self.into_iter().enumerate() {
            if i != 0 {
                write!(f, ", ")?;
            }

            t.typed_fmt(db, &(), f)?;
        }

        Ok(())
    }
}

impl TypedDisplay for Field {
    fn typed_fmt(&self, db: &dyn TypeDatabase, _: &(), f: &mut Formatter) -> Result {
        write!(f, "{} :: {}", self.name, Typed(db, &(), &self.ty))
    }
}

impl TypedDisplay for Ctnt {
    fn typed_fmt(&self, db: &dyn TypeDatabase, _: &(), f: &mut Formatter) -> Result {
        let file = db.module_tree(self.trait_.lib).file(self.trait_.module);
        let hir = db.module_hir(file);
        let def = hir.def(self.trait_);

        def.name().fmt(f)?;

        for ty in &self.tys {
            write!(f, " {}", Typed(db, &(), &ty))?;
        }

        Ok(())
    }
}

impl Display for TypeVar {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let mut num = if self.0.local_id.0 > i32::max_value as u32 {
            u32::max_value() - self.0.local_id.0
        } else {
            self.0.local_id.0
        };

        while num >= 26 {
            write!(f, "{}", (b'a' + (num % 26) as u8) as char)?;
            num /= 26;
        }

        write!(f, "{}", (b'a' + num as u8) as char)
    }
}

pub type Types = std::collections::HashMap<ir::HirId, Ty>;

impl TypedDisplay<Types> for ir::Body {
    fn typed_fmt(&self, db: &dyn TypeDatabase, tys: &Types, f: &mut Formatter) -> Result {
        for param in &self.params {
            write!(f, "{} ", Typed(db, tys, param))?;
        }

        writeln!(f, "=")?;
        write!(indent(f), "{}", Typed(db, tys, &self.value))
    }
}

impl TypedDisplay<Types> for ir::Param {
    fn typed_fmt(&self, db: &dyn TypeDatabase, tys: &Types, f: &mut Formatter) -> Result {
        write!(
            f,
            "($p{} :: {})",
            self.id.local_id.0,
            Typed(db, &(), &tys[&self.id])
        )
    }
}

impl TypedDisplay<Types> for ir::Guarded {
    fn typed_fmt(&self, db: &dyn TypeDatabase, tys: &Types, f: &mut Formatter) -> Result {
        match self {
            ir::Guarded::Unconditional(expr) => write!(f, "-> {}", Typed(db, tys, expr)),
            ir::Guarded::Guarded(_exprs) => unimplemented!(),
        }
    }
}

impl TypedDisplay<Types> for ir::Expr {
    fn typed_fmt(&self, db: &dyn TypeDatabase, tys: &Types, f: &mut Formatter) -> Result {
        let ty = &tys[&self.id];

        match &self.kind {
            ir::ExprKind::Error => write!(f, "{{error}} :: {}", Typed(db, &(), ty)),
            ir::ExprKind::Hole { name } => write!(f, "?{} :: {}", name, Typed(db, &(), ty)),
            ir::ExprKind::Int { val } => write!(f, "{} :: {}", val, Typed(db, &(), ty)),
            ir::ExprKind::Float { bits } => {
                write!(f, "{} :: {}", f64::from_bits(*bits), Typed(db, &(), ty))
            }
            ir::ExprKind::Ident { name, .. } => write!(f, "{} :: {}", name, Typed(db, &(), ty)),
            ir::ExprKind::Tuple { exprs } => {
                write!(f, "(")?;

                for (i, expr) in exprs.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }

                    write!(f, "{}", Typed(db, tys, expr))?;
                }

                write!(f, ") :: {}", Typed(db, &(), ty))
            }
            ir::ExprKind::App { base, args } => {
                write!(f, "({}", Typed(db, tys, &**base))?;

                for arg in args {
                    writeln!(f)?;
                    write!(indent(f), "({})", Typed(db, tys, arg))?;
                }

                write!(f, ") :: {}", Typed(db, &(), ty))?;

                Ok(())
            }
            ir::ExprKind::Field { base, field } => {
                write!(
                    f,
                    "({}).{} :: {}",
                    Typed(db, tys, &**base),
                    field,
                    Typed(db, &(), ty)
                )
            }
            ir::ExprKind::If { cond, then, else_ } => {
                writeln!(f, "(if {}", Typed(db, tys, &**cond))?;
                writeln!(indent(f), "then {}", Typed(db, tys, &**then))?;
                write!(
                    indent(f),
                    "else {}) :: {}",
                    Typed(db, tys, &**else_),
                    Typed(db, &(), ty)
                )
            }
            ir::ExprKind::Case { pred, arms } => {
                write!(f, "(case ")?;

                for (i, pred) in pred.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }

                    pred.typed_fmt(db, tys, f)?;
                }

                write!(f, " of")?;

                for arm in arms {
                    writeln!(f)?;
                    write!(indent(f), "{}", Typed(db, tys, arm))?;
                }

                write!(f, ") :: {}", Typed(db, &(), ty))
            }
            _ => unimplemented!(),
        }
    }
}

impl TypedDisplay<Types> for ir::CaseArm {
    fn typed_fmt(&self, db: &dyn TypeDatabase, tys: &Types, f: &mut Formatter) -> Result {
        for (i, pat) in self.pats.iter().enumerate() {
            if i != 0 {
                write!(f, ", ")?;
            }

            pat.typed_fmt(db, tys, f)?;
        }

        write!(f, " {}", Typed(db, tys, &self.val))
    }
}

impl TypedDisplay<Types> for ir::Pat {
    fn typed_fmt(&self, db: &dyn TypeDatabase, tys: &Types, f: &mut Formatter) -> Result {
        let ty = &tys[&self.id];

        match &self.kind {
            ir::PatKind::Error => write!(f, "{{error}} :: {}", Typed(db, &(), ty)),
            ir::PatKind::Bind { name, sub: None } => {
                write!(f, "{} :: {}", name, Typed(db, &(), ty))
            }
            _ => unimplemented!(),
        }
    }
}

fn indent<'a, W: Write>(f: &'a mut W) -> Indent<'a, W> {
    Indent(f, true, "    ")
}

struct Indent<'a, W: Write>(&'a mut W, bool, &'a str);

impl<'a, W: Write> Write for Indent<'a, W> {
    fn write_str(&mut self, s: &str) -> Result {
        for c in s.chars() {
            if c == '\n' {
                self.0.write_char(c)?;
                self.1 = true;
                continue;
            }

            if self.1 {
                self.0.write_str(self.2)?;
                self.1 = false;
            }

            self.0.write_char(c)?;
        }

        Ok(())
    }
}