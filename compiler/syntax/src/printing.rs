use crate::ast::*;
use std::fmt::{Display, Formatter, Result, Write};

impl Display for Package {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.module.fmt(f)
    }
}

impl Display for Module {
    fn fmt(&self, f: &mut Formatter) -> Result {
        for (i, item) in self.items.iter().enumerate() {
            if i != 0 {
                writeln!(f)?;
            }

            item.fmt(f)?;
        }

        Ok(())
    }
}

impl Display for Item {
    fn fmt(&self, f: &mut Formatter) -> Result {
        for attr in &self.attrs {
            writeln!(f, "{}", attr)?;
        }

        match &self.kind {
            ItemKind::Module { module } => {
                writeln!(f, "mod {} where", self.name)?;
                writeln!(indent(f), "{}", module)?;
                write!(f, "end")
            }
            ItemKind::Extern { abi, ty } => write!(f, "extern {}{}: {};", self.name, abi, ty),
            ItemKind::Func {
                generics,
                params,
                ret,
                body,
            } => {
                let ret = if let TypeKind::Infer = &ret.kind {
                    String::new()
                } else {
                    format!("-> {} ", ret)
                };

                write!(
                    f,
                    "fn{} {}({}) {}{}",
                    generics,
                    self.name,
                    list(params, ", "),
                    ret,
                    body
                )
            }
            ItemKind::Var {
                ty:
                    Type {
                        kind: TypeKind::Infer,
                        ..
                    },
                val: Some(val),
            } => write!(f, "var {} = {};", self.name, val),
            ItemKind::Var { ty, val: Some(val) } => {
                write!(f, "var {}: {} = {};", self.name, ty, val)
            }
            ItemKind::Var { ty, val: None } => write!(f, "var {}: {};", self.name, ty),
            ItemKind::Const {
                ty:
                    Type {
                        kind: TypeKind::Infer,
                        ..
                    },
                val,
            } => write!(f, "const {} = {};", self.name, val),
            ItemKind::Const { ty, val } => write!(f, "const {}: {} = {};", self.name, ty, val),
            ItemKind::Struct { generics, fields } => {
                writeln!(f, "struct {}{}", self.name, generics)?;

                for field in fields {
                    writeln!(indent(f), "{}", field)?;
                }

                write!(f, "end")
            }
            ItemKind::Enum { generics, variants } => {
                writeln!(f, "enum {}{}", self.name, generics)?;

                for variant in variants {
                    writeln!(indent(f), "{}", variant)?;
                }

                write!(f, "end")
            }
        }
    }
}

impl Display for Attribute {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match &self.kind {
            AttrKind::Doc(text) => write!(
                f,
                "{}",
                text.lines()
                    .enumerate()
                    .map(|(i, l)| format!("--|{}{}", if i == 0 { "" } else { " " }, l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            AttrKind::NoMangle => write!(f, "@no_mangle"),
            AttrKind::Lang(name) => write!(f, "@lang {}", name),
            AttrKind::Intrinsic => write!(f, "@intrinsic"),
            AttrKind::Main => write!(f, "@main"),
        }
    }
}

impl Display for Abi {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Abi::None => Ok(()),
            Abi::C => write!(f, "\"C\" "),
        }
    }
}

impl Display for Generics {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if self.params.is_empty() {
            Ok(())
        } else {
            write!(f, "({})", list(&self.params, ", "))
        }
    }
}

impl Display for Generic {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.name.fmt(f)
    }
}

impl Display for Param {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.name.fmt(f)?;

        if !matches!(&self.ty.kind, TypeKind::Infer) {
            write!(f, ": {}", self.ty)?;
        }

        Ok(())
    }
}

impl Display for StructField {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.name.fmt(f)?;

        if !matches!(&self.ty.kind, TypeKind::Infer) {
            write!(f, ": {}", self.ty)?;
        }

        Ok(())
    }
}

impl Display for EnumVariant {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.name.fmt(f)?;

        if let Some(fields) = &self.fields {
            write!(f, "({})", list(fields, ", "))?;
        }

        Ok(())
    }
}

impl Display for Block {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if self.stmts.is_empty() {
            return write!(f, "end");
        }

        writeln!(f)?;

        for stmt in &self.stmts {
            writeln!(indent(f), "{}", stmt)?;
        }

        write!(f, "end")
    }
}

impl Display for Stmt {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match &self.kind {
            StmtKind::Item(item) => item.fmt(f),
            StmtKind::Semi(expr) => write!(f, "{};", expr),
            StmtKind::Expr(expr) => expr.fmt(f),
        }
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if self.root {
            write!(f, "/")?;
        }

        write!(f, "{}", list(&self.segs, "/"))
    }
}

impl Display for PathSeg {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            PathSeg::Name(name) => name.fmt(f),
            PathSeg::Current => write!(f, "."),
            PathSeg::Parent => write!(f, ".."),
            PathSeg::Package => write!(f, "~"),
        }
    }
}

impl Display for Expr {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match &self.kind {
            ExprKind::Path { path } => path.fmt(f),
            ExprKind::Apply { expr, args } => write!(f, "{}.<{}>", expr, list(args, ", ")),
            ExprKind::Int { val } => val.fmt(f),
            ExprKind::Float { bits } => f64::from_bits(*bits).fmt(f),
            ExprKind::Char { val } => write!(f, "{:?}", val),
            ExprKind::String { val } => write!(f, "{:?}", val),
            ExprKind::Parens { inner } => write!(f, "({})", inner),
            ExprKind::Type { ty } => write!(f, "`{}`", ty),
            ExprKind::Array { exprs } => write!(f, "[{}]", list(exprs, ", ")),
            ExprKind::Tuple { exprs } => write!(f, "({})", list(exprs, ", ")),
            ExprKind::Init { fields } => write!(f, "{{ {} }}", list(fields, ", ")),
            ExprKind::Range { lo, hi } => write!(f, "{}..{}", lo, hi),
            ExprKind::Block { block } => write!(f, "do {}", block),
            ExprKind::Call { func, args } => write!(f, "{}({})", func, list(args, ", ")),
            ExprKind::MethodCall { obj, method, args } => {
                write!(f, "{}.{}({})", obj, method, list(args, ", "))
            }
            ExprKind::Field { obj, field } => write!(f, "{}.{}", obj, field),
            ExprKind::Index { list, index } => write!(f, "{}[{}]", list, index),
            ExprKind::Slice {
                list,
                low: Some(l),
                high: Some(h),
            } => write!(f, "{}[{}..{}]", list, l, h),
            ExprKind::Slice {
                list,
                low: Some(l),
                high: None,
            } => write!(f, "{}[{}..]", list, l),
            ExprKind::Slice {
                list,
                low: None,
                high: Some(h),
            } => write!(f, "{}[..{}]", list, h),
            ExprKind::Slice {
                list,
                low: None,
                high: None,
            } => write!(f, "{}[..]", list),
            ExprKind::Ref { expr } => write!(f, "&{}", expr),
            ExprKind::Deref { expr } => write!(f, "{}.*", expr),
            ExprKind::TypeOf { expr } => write!(f, "{}.type", expr),
            ExprKind::Cast { expr, ty } => write!(f, "{}.({})", expr, ty),
            ExprKind::Box { expr } => write!(f, "box {}", expr),
            ExprKind::Unbox { expr } => write!(f, "unbox {}", expr),
            ExprKind::Assign { lhs, rhs } => write!(f, "{} = {}", lhs, rhs),
            ExprKind::AssignOp { op, lhs, rhs } => write!(f, "{} {}= {}", lhs, op, rhs),
            ExprKind::BinOp { op, lhs, rhs } => write!(f, "{} {} {}", lhs, op, rhs),
            ExprKind::UnOp { op, rhs } => write!(f, "{}{}", op, rhs),
            ExprKind::IfElse {
                cond,
                then,
                else_: Some(else_),
            } => write!(f, "if {} {} else {}", cond, then, else_),
            ExprKind::IfElse {
                cond,
                then,
                else_: None,
            } => write!(f, "if {} {}", cond, then),
            ExprKind::While {
                label: Some(label),
                cond,
                body,
            } => write!(f, ":{} while {} {}", label, cond, body),
            ExprKind::While {
                label: None,
                cond,
                body,
            } => write!(f, "while {} {}", cond, body),
            ExprKind::Loop {
                label: Some(label),
                body,
            } => write!(f, ":{} loop {}", label, body),
            ExprKind::Loop { label: None, body } => write!(f, "loop {}", body),
            ExprKind::Break {
                label: Some(label),
                expr: Some(expr),
            } => write!(f, "break :{} {}", label, expr),
            ExprKind::Break {
                label: Some(label),
                expr: None,
            } => write!(f, "break :{}", label),
            ExprKind::Break {
                label: None,
                expr: Some(expr),
            } => write!(f, "break {}", expr),
            ExprKind::Break {
                label: None,
                expr: None,
            } => write!(f, "break"),
            ExprKind::Continue { label: Some(label) } => write!(f, "continue :{}", label),
            ExprKind::Continue { label: None } => write!(f, "continue"),
            ExprKind::Return { expr: Some(expr) } => write!(f, "return {}", expr),
            ExprKind::Return { expr: None } => write!(f, "return"),
            ExprKind::Defer { expr } => write!(f, "defer {}", expr),
        }
    }
}

impl Display for InitField {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{} = {}", self.name, self.value)
    }
}

impl Display for Arg {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if let Some(name) = &self.name {
            write!(f, "{} = ", name)?;
        }

        self.value.fmt(f)
    }
}

impl Display for BinOp {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Rem => write!(f, "%"),
            BinOp::Lt => write!(f, "<"),
            BinOp::Le => write!(f, "<="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Ge => write!(f, ">="),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::BitAnd => write!(f, "&"),
            BinOp::BitOr => write!(f, "|"),
            BinOp::BitXOr => write!(f, "^"),
            BinOp::Shl => write!(f, "<<"),
            BinOp::Shr => write!(f, ">>"),
            BinOp::And => write!(f, "and"),
            BinOp::Or => write!(f, "or"),
        }
    }
}

impl Display for UnOp {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            UnOp::Neg => write!(f, "-"),
            UnOp::Not => write!(f, "!"),
        }
    }
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match &self.kind {
            TypeKind::Infer => write!(f, "_"),
            TypeKind::Parens { inner } => write!(f, "({})", inner),
            TypeKind::Path { path } => path.fmt(f),
            TypeKind::Func { params, ret } => write!(f, "fn ({}) -> {}", list(params, ", "), ret),
            TypeKind::Ref { mut_: true, ty } => write!(f, "*mut {}", ty),
            TypeKind::Ref { mut_: false, ty } => write!(f, "*{}", ty),
            TypeKind::Array { of, len } => write!(f, "[{}; {}]", of, len),
            TypeKind::Slice { of } => write!(f, "[{}]", of),
            TypeKind::Tuple { tys } => write!(f, "({})", list(tys, ", ")),
            TypeKind::Subst { ty, args } => write!(f, "{}({})", ty, list(args, ", ")),
            TypeKind::Forall { gen, ty } => write!(f, "forall {}. {}", gen, ty),
        }
    }
}

impl Display for TypeParam {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}: {}", self.name, self.ty)
    }
}

fn list(i: impl IntoIterator<Item = impl Display>, sep: &str) -> String {
    i.into_iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(sep)
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