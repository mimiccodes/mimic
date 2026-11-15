//! Debug information (DWARF), for debug builds (G4).
//!
//! Every function the program defines gets a subprogram: the program's own
//! functions under their Mimic names (`main::fib`), at their line; what the
//! compiler makes (lambdas, kernels, thunks) as artificial ones in the same
//! file. Each expression sets the source location (line and column) of the
//! code made for it, so a debugger steps through `.mim` lines and a crash
//! shows them. Parameters and `let`s are described as variables
//! (`frame variable` shows them): numbers, truth values, lists of known
//! length and flat records (kept in registers) with their real types, every
//! other value as a `value` handle, which `mimic lldb` shows as Mimic
//! prints it.

use super::*;
use inkwell::debug_info::{AsDIScope, DIFile, DIFlags, DIFlagsConstants, DISubprogram, DIType, DWARFEmissionKind, DWARFSourceLanguage, DebugInfoBuilder};

pub(crate) struct Dbg<'ctx> {
    b: DebugInfoBuilder<'ctx>,
    cu: inkwell::debug_info::DICompileUnit<'ctx>,
    files: HashMap<usize, DIFile<'ctx>>,
    /// the subprogram of the function being built, and its file
    cur: Option<(DISubprogram<'ctx>, DIFile<'ctx>)>,
    types: HashMap<&'static str, DIType<'ctx>>,
}

/// The parts of a path: file name and directory.
fn split(p: &Path) -> (String, String) {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "main.mim".into());
    let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    (name, dir)
}

impl<'ctx> Dbg<'ctx> {
    pub(crate) fn new(module: &Module<'ctx>, ctx: &'ctx Context, main_file: &Path, optimized: bool) -> Self {
        let (name, dir) = split(main_file);
        let (b, cu) = module.create_debug_info_builder(
            true,
            DWARFSourceLanguage::C,
            &name,
            &dir,
            "mimic",
            optimized,
            "",
            0,
            "",
            DWARFEmissionKind::Full,
            0,
            false,
            false,
            "",
            "",
        );
        let i32t = ctx.i32_type();
        module.add_basic_value_flag("Debug Info Version", inkwell::module::FlagBehavior::Warning, i32t.const_int(3, false));
        module.add_basic_value_flag("Dwarf Version", inkwell::module::FlagBehavior::Warning, i32t.const_int(4, false));
        let mut types = HashMap::new();
        // DW_ATE_boolean 2, float 4, signed 5, address 1
        for (n, bits, enc) in [("i64", 64, 5), ("i32", 32, 5), ("f64", 64, 4), ("f32", 32, 4), ("bool", 8, 2)] {
            if let Ok(t) = b.create_basic_type(n, bits, enc, DIFlags::ZERO) {
                types.insert(n, t.as_type());
            }
        }
        // a handle: `value`, a pointer to the runtime's block (header and
        // elements); `mimic lldb` shows what it holds by that name
        // (an opaque 64-bit handle rather than a C pointer, so debuggers show
        // only what it holds, not an address)
        if let Ok(handle) = b.create_basic_type("mim_handle", 64, 7, DIFlags::ZERO) {
            let td = b.create_typedef(handle.as_type(), "value", cu.get_file(), 0, cu.as_debug_info_scope(), 64);
            types.insert("value", td.as_type());
        }
        Dbg { b, cu, files: HashMap::new(), cur: None, types }
    }

    pub(crate) fn finalize(&self) {
        self.b.finalize();
    }
}

impl<'ctx> Cg<'ctx, '_> {
    fn dbg_file(&mut self, m: ModId) -> Option<DIFile<'ctx>> {
        let fi = self.p.mods[m].file?;
        let d = self.dbg.as_mut()?;
        if let Some(f) = d.files.get(&fi) {
            return Some(*f);
        }
        let (name, dir) = split(&self.p.files[fi].path);
        let f = d.b.create_file(&name, &dir);
        d.files.insert(fi, f);
        Some(f)
    }

    /// Start describing `fv` (being built now): a subprogram named `name` at
    /// `line` of module `m`'s file (or of the current function's file).
    pub(crate) fn dbg_enter(&mut self, fv: FunctionValue<'ctx>, name: &str, m: Option<ModId>, line: u32, artificial: bool) {
        if self.dbg.is_none() {
            return;
        }
        let file = match m.and_then(|m| self.dbg_file(m)) {
            Some(f) => f,
            None => match self.dbg.as_ref().and_then(|d| d.cur.map(|(_, f)| f)) {
                Some(f) => f,
                None => {
                    self.b.unset_current_debug_location();
                    return;
                }
            },
        };
        let optimized = self.opt_level >= 2;
        let d = self.dbg.as_mut().unwrap();
        let ty = d.b.create_subroutine_type(file, None, &[], DIFlags::ZERO);
        // (no linkage name: debuggers then show the Mimic name, `main::area`)
        let _ = fv.get_name();
        let flags = if artificial { DIFlags::ARTIFICIAL } else { DIFlags::ZERO };
        let sp = d.b.create_function(d.cu.as_debug_info_scope(), name, None, file, line, ty, true, true, line, flags, optimized);
        fv.set_subprogram(sp);
        d.cur = Some((sp, file));
        let loc = d.b.create_debug_location(self.ctx, line, 0, sp.as_debug_info_scope(), None);
        self.b.set_current_debug_location(loc);
    }

    /// Code made from now on is at `span` (of the current function's file).
    pub(crate) fn dbg_at(&self, span: mimic_syntax::Span) {
        let Some(d) = self.dbg.as_ref() else { return };
        let Some((sp, _)) = d.cur else { return };
        if span.line == 0 {
            return;
        }
        let loc = d.b.create_debug_location(self.ctx, span.line, span.col, sp.as_debug_info_scope(), None);
        self.b.set_current_debug_location(loc);
    }

    /// No function is being described (code made now has no location).
    pub(crate) fn dbg_leave(&mut self) {
        if let Some(d) = self.dbg.as_mut() {
            d.cur = None;
            self.b.unset_current_debug_location();
        }
    }

    /// The state to come back to after building another function.
    pub(crate) fn dbg_save(&self) -> Option<(Option<(DISubprogram<'ctx>, DIFile<'ctx>)>, Option<inkwell::debug_info::DILocation<'ctx>>)> {
        let d = self.dbg.as_ref()?;
        Some((d.cur, self.b.get_current_debug_location()))
    }

    pub(crate) fn dbg_restore(&mut self, s: Option<(Option<(DISubprogram<'ctx>, DIFile<'ctx>)>, Option<inkwell::debug_info::DILocation<'ctx>>)>) {
        let Some((cur, loc)) = s else { return };
        if let Some(d) = self.dbg.as_mut() {
            d.cur = cur;
        }
        match loc {
            Some(l) => self.b.set_current_debug_location(l),
            None => self.b.unset_current_debug_location(),
        }
    }

    /// Describe the variable `name` of type `t` kept at `slot` (a parameter
    /// when `arg` is its 1-based position).
    pub(crate) fn dbg_var(&self, name: &str, slot: PointerValue<'ctx>, t: Ty, line: u32, arg: Option<u32>) {
        let Some(d) = self.dbg.as_ref() else { return };
        let Some((sp, file)) = d.cur else { return };
        let ty = match self.dbg_type(t, file, 0) {
            Some((ty, ..)) => ty,
            None if self.repr(t).is_pointer_type() => match d.types.get("value") {
                Some(v) => *v,
                None => return,
            },
            None => return,
        };
        let Some(bb) = self.b.get_insert_block() else { return };
        let scope = sp.as_debug_info_scope();
        let var = match arg {
            Some(n) => d.b.create_parameter_variable(scope, name, n, file, line, ty, true, DIFlags::ZERO),
            None => d.b.create_auto_variable(scope, name, file, line, ty, true, DIFlags::ZERO, 0),
        };
        let loc = d.b.create_debug_location(self.ctx, line.max(1), 0, scope, None);
        d.b.insert_declare_at_end(slot, Some(var), Some(d.b.create_expression(vec![])), loc, bb);
    }

    /// The debug type of a value kept in registers, with its size and
    /// alignment in bits: a number, a truth value, a list of known length
    /// (an array of its elements) or a flat record (a struct with the
    /// record's field names, laid out as LLVM lays out its struct).
    fn dbg_type(&self, t: Ty, file: DIFile<'ctx>, depth: u32) -> Option<(DIType<'ctx>, u64, u32)> {
        let d = self.dbg.as_ref()?;
        let scalar = |k: &str, bits: u64| d.types.get(k).map(|t| (*t, bits, bits.min(64) as u32));
        match t {
            Ty::I64 => return scalar("i64", 64),
            Ty::I32 => return scalar("i32", 32),
            Ty::F64 => return scalar("f64", 64),
            Ty::F32 => return scalar("f32", 32),
            Ty::Bool => return scalar("bool", 8),
            _ => {}
        }
        if let Some((el, n)) = fixed(t) {
            let (et, bits, align) = self.dbg_type(Ty::of_el(el), file, depth + 1)?;
            let arr = d.b.create_array_type(et, bits * n as u64, align, &[0..n as i64]);
            return Some((arr.as_type(), bits * n as u64, align));
        }
        let fields = self.flat(t)?;
        if depth > 4 {
            return None;
        }
        let Ty::Obj(id) = t else { return None };
        let tid = types::obj_tid(id);
        let td = &self.p.types[tid];
        let mimic_syntax::ast::TypeKind::Record(decl) = &td.decl.kind else { return None };
        let scope = d.cu.as_debug_info_scope();
        let (mut offset, mut align_all) = (0u64, 8u32);
        let mut members = Vec::new();
        for ((fname, _, _), ft) in decl.iter().zip(&fields) {
            let (fty, bits, align) = self.dbg_type(*ft, file, depth + 1)?;
            offset = offset.div_ceil(align as u64) * align as u64;
            members.push(d.b.create_member_type(scope, fname, file, td.decl.span.line, bits, align, offset, DIFlags::ZERO, fty).as_type());
            offset += bits;
            align_all = align_all.max(align);
        }
        let size = offset.div_ceil(align_all as u64) * align_all as u64;
        let st = d.b.create_struct_type(scope, &td.name, file, td.decl.span.line, size, align_all, DIFlags::ZERO, None, &members, 0, None, &td.name);
        Some((st.as_type(), size, align_all))
    }
}
