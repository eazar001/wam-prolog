#![allow(dead_code)]
#![allow(unused)]
#![allow(clippy::new_without_default)]

pub mod ast;

use self::Cell::*;
use self::Store::*;
use self::Register::*;
use self::Mode::{Read, Write};
use self::ast::*;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter, Debug};
use std::cmp::Ordering;
use env_logger;
use log::{info, warn, error, debug, trace, Level};
use log::Level::*;
use lalrpop_util::lalrpop_mod;
use std::hash::Hash;

lalrpop_mod!(pub parser);


// heap address represented as usize that corresponds to the vector containing cell data
type HeapAddress = usize;
type Address =  usize;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum Register {
    // temporary
    X(usize),
    // permanent
    Y(usize)
}

type FunctorArity = usize;
type FunctorName = String;
// the "global stack"
type Heap = Vec<Cell>;
type TermMap = HashMap<Term, Register>;
type RegisterMap = HashMap<Register, Term>;
type TermSet = HashSet<Term>;
type Instructions = Vec<Instruction>;
type QueryBindings = Vec<String>;
type ProgramBindings = Vec<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    PutStructure(Functor, Register),
    GetStructure(Functor, Register),
    SetVariable(Register),
    UnifyVariable(Register),
    SetValue(Register),
    UnifyValue(Register),
    PutVariable(Register, Register),
    PutValue(Register, Register),
    GetValue(Register, Register),
    GetVariable(Register, Register),
    Allocate(usize),
    Deallocate,
    Call(Functor),
    Proceed
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct Functor(pub FunctorName, pub FunctorArity);

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum Cell {
    Str(HeapAddress),
    Ref(HeapAddress),
    Func(Functor)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Store {
    HeapAddr(HeapAddress),
    Register(Register)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Mode {
    Read,
    Write
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Code {
    code_area: Instructions,
    code_address: HashMap<Functor, usize>
}

// Environment stack frames
#[derive(Debug, Clone, Eq, PartialEq)]
enum Frame {
    Code(Address),
    Cell(Cell)
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Registers {
    // the "h" counter contains the location of the next cell to be pushed onto the heap
    h: HeapAddress,
    // variable register mapping a variable to cell data (x-register)
    x: Vec<Option<Cell>>,
    // subterm register containing heap address of next subterm to be matched (s-register)
    s: Address,
    // program/instruction counter, containing address of the next instruction to be executed
    p: Address,
    // address of the next instruction in the code area to follow up after successful return from a call
    cp: Address,
    // address of the latest environment on top of the stack
    e: Address
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Machine {
    heap: Heap,
    // the "push-down-list" contains StoreAddresses and serves as a unification stack
    pdl: Vec<Store>,
    code: Code,
    stack: Vec<Option<Frame>>,
    registers: Registers,
    mode: Mode,
    fail: bool,
}

impl Display for Cell {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Ref(a) => Ok(write!(f, "{:?}", Ref(*a))?),
            Str(a) => Ok(write!(f, "{:?}", Str(*a))?),
            Func(f1) => Ok(write!(f, "Functor({})", f1)?)
        }
    }
}

impl Display for Functor {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        Ok(write!(f, "{}/{}", self.name(), self.arity())?)
    }
}

impl From<&str> for Functor {
    fn from(s: &str) -> Functor {
        let v: Vec<&str> = s.split('/').collect();

        assert_eq!(v.len(), 2);

        Functor(String::from(v[0]), String::from(v[1]).parse().unwrap())
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Cell {
        Func(Functor::from(s))
    }
}

impl Functor {
    pub fn name(&self) -> &str {
        &self.0
    }

    pub fn arity(&self) -> usize {
        self.1
    }
}

impl Machine {
    pub fn new() -> Machine {
        Machine {
            heap: Heap::new(),
            pdl: Vec::new(),
            code: Code::new(),
            stack: Vec::new(),
            registers: Registers::new(),
            mode: Read,
            fail: false
        }
    }

    fn execute(&mut self, instruction: &Instruction) {
        match instruction {
            Instruction::PutStructure(f, x) => self.put_structure(f.clone(), *x),
            Instruction::GetStructure(f, x) => self.get_structure(f.clone(), *x),
            Instruction::SetValue(x) => self.set_value(*x),
            Instruction::UnifyValue(x) => self.unify_value(*x),
            Instruction::SetVariable(x) => self.set_variable(*x),
            Instruction::UnifyVariable(x) => self.unify_variable(*x),
            Instruction::PutVariable(x, a) => self.put_variable(*x, *a),
            Instruction::PutValue(x, a) => self.put_value(*x, *a),
            Instruction::GetValue(x, a) => self.get_value(*x, *a),
            Instruction::GetVariable(x, a) => self.get_variable(*x, *a),
            Instruction::Allocate(n) => self.allocate(*n),
            Instruction::Deallocate => self.deallocate(),
            Instruction::Call(f) => self.call(f),
            Instruction::Proceed => self.proceed()
        }
    }

    fn push_instruction(&mut self, instruction: Instruction) {
        self.code.code_area.push(instruction);
    }

    fn push_instructions(&mut self, fact: &Functor, instructions: &[Instruction]) {
        self.push_code_address(fact);
        for instruction in instructions {
            self.push_instruction(instruction.clone());
        }
    }

    fn execute_instructions(&mut self, fact: &Functor) {
        let mut a = self.get_code_address(fact);
        let instructions = self.get_code().clone();

        while a < instructions.len() {
            println!("{:?}", instructions[a]);
            match &instructions[a] {
                instruction@Instruction::Proceed | instruction@Instruction::Call(_) => {
                    self.execute(instruction);
                    break
                },
                instruction => self.execute(instruction)
            }

            a += 1;
        }
    }

    pub fn get_code(&self) -> &Instructions {
        &self.code.code_area
    }

    fn push_code_address(&mut self, fact: &Functor) {
        let a = self.code.code_area.len();
        self.code.code_address.insert(fact.clone(), a);
    }

    fn get_code_address(&self, fact: &Functor) -> usize {
        *self.code.code_address.get(fact).unwrap()
    }

    fn get_e(&self) -> usize {
        self.registers.e
    }

    fn set_e(&mut self, value: usize) {
        self.registers.e = value;
    }

    pub fn get_heap(&self) -> &Heap {
        &self.heap
    }

    pub fn allocate(&mut self, n: usize) {
        let e = self.get_e();
        self.stack.resize(e+n+5, None);
        self.stack[e+2] = Some(Frame::Code(n));
        let temp = match &self.stack[e+2] {
            Some(Frame::Code(a)) => a,
            _ => panic!("address retrieval error")
        };

        let new_e = e + temp + 3;

        self.stack[new_e] = Some(Frame::Code(e));
        self.stack[new_e+1] = Some(Frame::Code(self.get_cp()));
        self.set_e(new_e);
        self.set_p(self.get_p() + 1);

        println!("{:?}", self.stack);
        println!("{:?}", self.code.code_address);
        println!("{:?}", self.code.code_area);
    }

    pub fn deallocate(&mut self) {
        let stack = &self.stack;
        let e = self.get_e();
        let new_p = match &stack[e+1] {
            Some(Frame::Code(a)) => *a,
            _ => panic!("address retrieval error")
        };

        let new_e = match &stack[e] {
            Some(Frame::Code(a)) => *a,
            _ => panic!("address retrieval error")
        };

        self.set_p(new_p);
        self.set_e(new_e);
    }

    pub fn get_x_registers(&self) -> &Vec<Option<Cell>> {
        &self.registers.x
    }

    fn push_heap(&mut self, cell: Cell) {
        self.heap.push(cell);
    }

    pub fn is_true(&self) -> bool {
        !self.fail
    }

    pub fn is_false(&self) -> bool {
        self.fail
    }

    pub fn get_register(&self, register: Register) -> Option<&Cell> {
        match register {
            Register::X(xi) => self.get_x(xi),
            Register::Y(yi) => self.get_y(yi)
        }
    }

    pub fn get_x(&self, xi: usize) -> Option<&Cell> {
        self.get_x_registers()[xi-1].as_ref()
    }

    pub fn get_y(&self, yi: usize) -> Option<&Cell> {
        let offset = self.get_e() + 3;
        let stack = &self.stack;

        match &stack[offset+yi] {
            Some(Frame::Cell(c)) => Some(c),
            _ => panic!("error retrieving cell-data from permanent register")
        }
    }

    fn insert_register(&mut self, register: Register, cell: Cell) {
        match register {
            Register::X(xi) => self.insert_x(xi, cell),
            Register::Y(yi) => self.insert_y(yi, cell)
        }
    }

    fn insert_y(&mut self, yi: usize, cell: Cell) {
        let offset = self.get_e() + 3;
        self.stack[offset+yi] = Some(Frame::Cell(cell));
    }

    fn insert_x(&mut self, xi: usize, cell: Cell) {
        if xi > self.registers.x.len() {
            self.registers.x.resize(xi, None);
        }

        self.registers.x[xi-1] = Some(cell);
    }

    fn get_s(&self) -> HeapAddress {
        self.registers.s
    }

    fn inc_s(&mut self, value: usize) {
        self.registers.s += value;
    }

    fn set_s(&mut self, value: usize) {
        self.registers.s = value;
    }

    fn set_fail(&mut self, value: bool) {
        self.fail = value;
    }

    fn get_h(&self) -> usize {
        self.registers.h
    }

    fn get_p(&self) -> usize {
        self.registers.p
    }

    fn set_p(&mut self, value: usize) {
        self.registers.p = value;
    }

    fn get_cp(&self) -> usize {
        self.registers.cp
    }

    fn set_cp(&mut self, value: usize) {
        self.registers.cp = value;
    }

    fn inc_h(&mut self, value: usize) {
        self.registers.h += value;
    }

    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    fn empty_pdl(&mut self) -> bool {
        self.pdl.is_empty()
    }

    fn push_pdl(&mut self, address: Store) {
        self.pdl.push(address);
    }

    fn pop_pdl(&mut self) -> Option<Store> {
        self.pdl.pop()
    }

    fn put_structure(&mut self, f: Functor, xi: Register) {
        let h = self.get_h();
        self.push_heap(Str(h+1));
        self.push_heap(Func(f));
        self.insert_register(xi, Str(h+1));
        self.inc_h(2);
    }

    fn put_variable(&mut self, xn: Register, ai: Register) {
        let h = self.get_h();
        self.push_heap(Ref(h));
        self.insert_register(xn, Ref(h));
        self.insert_register(ai, Ref(h));
        self.inc_h(1);
    }

    fn put_value(&mut self, xn: Register, ai: Register) {
        self.insert_register(ai, self.get_register(xn).cloned().unwrap());
    }

    fn set_variable(&mut self, xi: Register) {
        let h = self.get_h();
        self.push_heap(Ref(h));
        self.insert_register(xi, Ref(h));
        self.inc_h(1);
    }

    fn set_value(&mut self, xi: Register) {
        self.push_heap(self.get_register(xi).cloned().unwrap());
        self.inc_h(1);
    }

    fn deref(&self, address: Store) -> Store {
        let mut address = address;
        let start_address = address;

        loop {
            let (cell, a) = match address {
                HeapAddr(addr) => (&self.heap[addr], addr),
                Store::Register(r) => {
                    let addr = r.address();
                    let e = &format!("Illegal access: register {:?}, does not exist", addr);
                    let c = self.get_register(r).expect(e);

                    (c, addr)
                }
            };

            match cell {
                Ref(value) => {
                    if *value != a {
                        // keep following the reference chain
                        address = HeapAddr(*value);
                    } else {
                        // ref cell is unbound return the address
                        return address
                    }
                },
                Str(_) => {
                    return address
                },
                Func(_) => {
                           return address
                }
            }
        }
    }

    fn call(&mut self, f: &Functor) {
        let a = self.get_code_address(f);
        let p = self.get_p();
        self.set_cp(p+1);
        self.set_p(a);
    }

    fn proceed(&mut self) {
        let cp = self.get_cp();
        self.set_p(cp);
    }

    fn get_variable(&mut self, xn: Register, ai: Register) {
        self.insert_register(xn, self.get_register(ai).cloned().unwrap());
    }

    fn get_value(&mut self, xn: Register, ai: Register) {
        self.unify(Store::Register(xn), Store::Register(ai));
    }

    fn get_structure(&mut self, f: Functor, xi: Register) {
        let (cell, address) = match self.deref(Store::Register(xi)) {
            HeapAddr(addr) => (&self.heap[addr], addr),
            Store::Register(r) => (self.get_register(xi).unwrap(), r.address())
        };

        match cell.clone() {
            Ref(_) => {
                let h = self.get_h();

                self.push_heap(Str(h+1));
                self.push_heap(Func(f.clone()));
                self.bind(HeapAddr(address), HeapAddr(h));

                self.inc_h(2);
                self.set_mode(Write);
            },
            Str(a) => {
                match self.heap[a] {
                    Func(ref functor) => {
                        if *functor == f {
                            self.set_s(a+1);
                            self.set_mode(Read);
                        } else {
                            self.set_fail(true);
                        }
                    }
                    _ => panic!()
                }
            },
            Func(_) => {
                self.set_fail(true);
            }
        }
    }

    fn unify_variable(&mut self, xi: Register) {
        match self.mode {
            Read => {
                let s = self.get_s();

                self.insert_register(xi, self.heap[s].clone());
            },
            Write => {
                let h = self.get_h();

                self.push_heap(Ref(h));
                self.insert_register(xi, Ref(h));
                self.inc_h(1);
            }
        }

        self.inc_s(1);
    }

    fn unify_value(&mut self, xi: Register) {
        match self.mode {
            Read => {
                let s = self.get_s();

                self.unify(Store::Register(xi), Store::HeapAddr(s))
            },
            Write => {
                self.push_heap(self.get_register(xi).cloned().unwrap());
                self.inc_h(1);
            }
        }

        self.inc_s(1);
    }

    pub fn unify(&mut self, a1: Store, a2: Store) {
        self.push_pdl(a1);
        self.push_pdl(a2);

        self.set_fail(false);

        while !(self.empty_pdl() || self.fail) {
            let (a1, a2) = (self.pop_pdl().unwrap(), self.pop_pdl().unwrap());

            let d1 = self.deref(a1);
            let d2 = self.deref(a2);

            if d1 != d2 {
                let c1 = self.get_store_cell(d1);
                let c2 = self.get_store_cell(d2);

                if c1.is_ref() || c2.is_ref() {
                    self.bind(d1, d2);
                } else {
                    let (v1, v2) = (c1.address().unwrap(), c2.address().unwrap());
                    let (f1, f2) = (self.get_functor(c1), self.get_functor(c2));

                    if f1 == f2 {
                        let n1 = f1.arity();

                        for i in 1..=n1 {
                            self.push_pdl(HeapAddr(v1+i));
                            self.push_pdl(HeapAddr(v2+i));
                        }
                    } else {
                        self.set_fail(true);
                    }
                }
            }
        }
    }

    // extracts functor only if cell is a structure or a functor cell
    fn get_functor<'a>(&'a self, cell: &'a Cell) -> &'a Functor {
        match cell {
            Str(addr) => {
                if let Func(f) = &self.heap[*addr] {
                    &f
                } else {
                    error!("encountered a structure that doesn't point to a functor");
                    panic!("invalid cell: structure cell pointing to non-functor data")
                }
            },
            Func(f) => {
                warn!("accessing a functor from a functor-cell, but this normally shouldn't happen");
                f
            },
            Ref(_) => {
                error!("tried getting a functor from a ref-cell");
                panic!("invalid cell-type for functor retrieval used");
            }
        }
    }

    fn get_store_cell(&self, address: Store) -> &Cell {
        match address {
            HeapAddr(addr) => &self.heap[addr],
            Register(r) => self.get_register(r).unwrap()
        }
    }

    fn bind(&mut self, a1: Store, a2: Store) {
        let (c1, c2) = (self.get_store_cell(a1), self.get_store_cell(a2));
        let (a1, a2) = (c1.address().unwrap(), c2.address().unwrap());

        if c1.is_ref() && (!c2.is_ref() || a2 < a1) {
            self.heap[a1] = c2.clone();
        } else {
            self.heap[a2] = c1.clone();
        }
    }
}

impl Register {
    fn is_x(&self) -> bool {
        if let Register::X(_) = self {
            return true
        }

        false
    }

    fn is_y(&self) -> bool {
        !self.is_x()
    }

    fn address(&self) -> usize {
        match self {
            Register::X(a) => *a,
            Register::Y(a) => *a
        }
    }
}

impl Registers {
    fn new() -> Registers {
        Registers {
            h: 0,
            x: Vec::new(),
            s: 0,
            p: 0,
            cp: 0,
            e: 0
        }
    }
}

impl Cell {
    fn is_ref(&self) -> bool {
        if let Ref(_) = self {
            return true
        }

        false
    }

    fn is_str(&self) -> bool {
        if let Str(_) = self {
            return true
        }

        false
    }

    fn is_func(&self) -> bool {
        if let Func(_) = self {
            return true
        }

        false
    }

    pub fn address(&self) -> Option<HeapAddress> {
        match self {
            Str(addr) => Some(*addr),
            Ref(addr) => Some(*addr),
            Func(_) => None
        }
    }
}

impl Store {
    fn is_heap(&self) -> bool {
        if let HeapAddr(_) = self {
            return true
        }

        false
    }

    fn is_x(&self) -> bool {
        if let Store::Register(Register::X(_)) = self {
            return true
        }

        false
    }

    fn is_y(&self) -> bool {
        if let Store::Register(Register::Y(_)) = self {
            return true
        }

        false
    }

    fn address(&self) -> usize {
        match self {
            HeapAddr(addr) => *addr,
            Store::Register(r) => r.address()
        }
    }
}

impl Code {
    fn new() -> Self {
        Code {
            code_area: Vec::new(),
            code_address: HashMap::new()
        }
    }
}

// TODO: make this iterative
fn allocate_query_registers(compound: &Compound, x: &mut usize, m: &mut TermMap, seen: &mut TermSet, instructions: &mut Instructions) {
    let term = Term::Compound(compound.clone());

    if !m.contains_key(&term) {
        m.insert(term, X(*x));
        *x += 1;
    }

    for t in &compound.args {
        if !m.contains_key(&t) {
            m.insert(t.clone(), X(*x));
            *x += 1;
        }
    }

    for t in &compound.args {
        if let Term::Compound(ref c) = t {
            allocate_query_registers(c, x, m, seen, instructions);
        }
    }

    let f = Functor(compound.name.clone(), compound.arity);
    let t = Term::Compound(compound.clone());

    instructions.push(Instruction::PutStructure(f, *m.get(&t).unwrap()));
    seen.insert(t);

    for t in &compound.args {
        if !seen.contains(t) {
            instructions.push(Instruction::SetVariable(*m.get(t).unwrap()));
            seen.insert(t.clone());
        } else {
            instructions.push(Instruction::SetValue(*m.get(t).unwrap()));
        }
    }
}

// TODO: make this iterative
fn allocate_program_registers(root: bool, compound: &Compound, x: &mut usize, m: &mut TermMap, seen: &mut TermSet, arg_instructions: &mut Instructions, instructions: &mut Instructions) {
    let term = Term::Compound(compound.clone());

    if !m.contains_key(&term) {
        m.insert(term, X(*x));
        *x += 1;
    }

    for t in &compound.args {
        if !m.contains_key(&t) {
            m.insert(t.clone(), X(*x));
            *x += 1;
        }
    }

    let f = Functor(compound.name.clone(), compound.arity);
    let t = Term::Compound(compound.clone());

    if root {
        arg_instructions.push(Instruction::GetStructure(f, *m.get(&t).unwrap()));
    } else {
        instructions.push(Instruction::GetStructure(f, *m.get(&t).unwrap()));
    }

    seen.insert(t);

    for t in &compound.args {
        if !seen.contains(t) {
            if root {
                arg_instructions.push(Instruction::UnifyVariable(*m.get(t).unwrap()));
            } else {
                instructions.push(Instruction::UnifyVariable(*m.get(t).unwrap()));
            }

            seen.insert(t.clone());
        } else if root {
            arg_instructions.push(Instruction::UnifyValue(*m.get(t).unwrap()));
        } else {
            instructions.push(Instruction::UnifyValue(*m.get(t).unwrap()));
        }
    }

    for t in &compound.args {
        if let Term::Compound(ref c) = t {
            allocate_program_registers(false, c, x, m, seen, arg_instructions, instructions);
        }
    }
}

fn compile_query<T: Structuralize>(term: &T, m: &mut TermMap, seen: &mut TermSet) -> Instructions {
    let mut instructions = Vec::new();

    let compound = term.structuralize().unwrap();

    for (i, arg) in compound.args.iter().enumerate() {
        let a = i + 1;
        let mut x = a + compound.arity;

        if let Term::Var(_) = arg {
            if !seen.contains(arg) {
                if m.contains_key(arg) {
                    instructions.push(Instruction::PutVariable(*m.get(&arg).unwrap(), X(a)));
                } else {
                    instructions.push(Instruction::PutVariable(X(x), X(a)));
                    m.insert(arg.clone(), X(x));
                }

                seen.insert(arg.clone());
            } else {
                instructions.push(Instruction::PutValue(*m.get(arg).unwrap(), X(a)));
            }
        } else {
            m.insert(arg.clone(), X(a));
            seen.insert(arg.clone());
            allocate_query_registers(&arg.structuralize().unwrap(), &mut x, m, seen, &mut instructions);
        }
    }

    instructions.push(Instruction::Call(Functor(compound.name.clone(), compound.arity)));

    instructions
}

fn compile_fact<T: Structuralize>(term: &T, m: &mut TermMap, seen: &mut HashSet<Term>) -> Instructions {
    let mut arg_instructions = Vec::new();
    let mut instructions = Vec::new();

    let compound = term.structuralize().unwrap();

    for (i, arg) in compound.args.iter().enumerate() {
        let a = i + 1;
        let mut x = a + compound.arity;

        if let Term::Var(_) = arg {
            if !seen.contains(arg) {
                if m.contains_key(arg) {
                    arg_instructions.push(Instruction::GetVariable(*m.get(&arg).unwrap(), X(a)));
                } else {
                    arg_instructions.push(Instruction::GetVariable(X(x), X(a)));
                    m.insert(arg.clone(), X(x));
                }

                seen.insert(arg.clone());
            } else {
                arg_instructions.push(Instruction::GetValue(*m.get(arg).unwrap(), X(a)));
            }
        } else {
            m.insert(arg.clone(), X(a));
            seen.insert(arg.clone());
            allocate_program_registers(true, &arg.structuralize().unwrap(), &mut x, m, seen, &mut arg_instructions, &mut instructions);
        }
    }

    instructions.push(Instruction::Proceed);
    arg_instructions.extend_from_slice(&instructions);

    arg_instructions
}

fn find_variables(term: &Term, vars: &mut Vec<Var>) {
    if let Term::Compound(c) = term {
        for arg in &c.args {
            if let Term::Var(v) = arg {
                vars.push(v.clone());
            } else if let Term::Compound(Compound {name, arity, .. }) = arg {
                if *arity > 0 {
                    find_variables(arg, vars);
                }
            }
        }
    }
}

fn find_variable_positions(all_vars: &[Var]) -> Vec<Term> {
    let mut perm_vars = Vec::new();

    for var in all_vars {
        let t = Term::Var(var.clone());
        if !perm_vars.contains(&t) {
            perm_vars.push(t);
        }
    }

    perm_vars
}

fn collect_permanent_variables(rule: &Rule) -> TermMap {
    let Rule { head, body } = rule;
    let mut vars = Vec::new();
    let mut all_vars = Vec::new();
    let head = Term::Compound(head.structuralize().unwrap());
    let mut counts = HashMap::new();

    find_variables(&head, &mut vars);
    find_variables(&Term::Compound(body[0].clone()), &mut vars);

    for head_var in &vars {
        counts.insert(head_var.clone(), 1);
    }

    vars.clear();

    for body_term in &body[1..] {
        find_variables(&Term::Compound(body_term.clone()), &mut vars);
    }

    for body_var in &vars {
        match counts.get(body_var).cloned() {
            Some(c) => counts.insert(body_var.clone(), c+1),
            None => counts.insert(body_var.clone(), 1)
        };
    }

    let vars: Vec<Term> = counts.iter()
        .filter(|(v,c)| **c > 1)
        .map(|(v,c)| Term::Var(v.clone()))
        .collect();

    find_variables(&head, &mut all_vars);

    for body_term in body {
        find_variables(&Term::Compound(body_term.clone()), &mut all_vars);
    }

    let mut perm_vars = find_variable_positions(&all_vars);
    let mut temp = Vec::new();

    for term in &perm_vars {
        if vars.contains(term) && !temp.contains(term) {
            temp.push(term.clone());
        }
    }

    let mut vars = HashMap::new();

    for (i, term) in temp.iter().enumerate() {
        vars.insert(term.clone(), Y(i+1));
    }

    vars
}

fn compile_rule(rule: &Rule, m: &mut TermMap, seen: &mut TermSet) -> Instructions {
    let (mut instructions, mut body_instructions) = (Vec::new(), Vec::new());
    let y_map = collect_permanent_variables(rule);
    let n = y_map.len();

    m.extend(y_map);

    let Rule { head: term, body } = rule;
    let head = Term::Compound(term.clone());
    let head_instructions = compile_fact(&head, m, seen);
    let head_slice = &head_instructions[..head_instructions.len()-1];

    let mut head_instructions = vec![Instruction::Allocate(n)];
    head_instructions.extend_from_slice(head_slice);

    for body_term in body {
        let body_term_instructions = compile_query(body_term, m, seen);
        body_instructions.extend(body_term_instructions);
    }

    body_instructions.push(Instruction::Deallocate);
    head_instructions.extend(instructions);
    head_instructions.extend(body_instructions);
    let instructions = head_instructions;

    println!("{:?}", instructions);

    instructions
}

pub fn query(m: &mut Machine, q: &str) -> HashMap<Cell, Term> {
    let e = parser::ExpressionParser::new();
    let mut query = e.parse(q).unwrap();
    let mut seen = HashSet::new();
    let mut map = HashMap::new();

    if let t@Term::Compound(_) | t@Term::Atom(_) = &mut query {
        let instructions = compile_query(t, &mut map, &mut seen);
        let mut output = HashMap::new();

        {
            let compound = t.structuralize().unwrap();
            let name = compound.name;
            let arity = compound.arity;
            let query_functor = Functor(name, arity);
            m.push_instructions(&query_functor, &instructions);
            m.execute_instructions(&query_functor);
        }

        for (term, x) in &map {
            output.insert(m.get_register(*x).cloned().unwrap(), term.clone());
        }

        output
    } else {
        panic!("not supported yet")
    }
}

// execute a query against the program term and display the results of the bindings (if any)
pub fn run_query(m: &mut Machine, q: &str, p: &str) -> (QueryBindings, ProgramBindings) {
    let query_map = query(m, q);
    let program_map = program(m, p);

    let mut display_vec = Vec::new();
    let mut query_bindings = Vec::new();
    let mut program_bindings = Vec::new();

    display_vec.extend(query_map);

    for (cell, term) in &display_vec {
        match cell {
            Cell::Ref(a) | Cell::Str(a) => {
                if let ast::Term::Var(_) = term {
                    let mut buffer = String::new();
                    resolve_term(&m, *a, &display_vec, &mut buffer);

                    if buffer != term.to_string() {
                        query_bindings.push(format!("{} = {}", term, buffer));
                    }
                }
            },
            _ => ()
        }
    }

    display_vec.extend(program_map);

    for (cell, term) in &display_vec {
        match cell {
            Cell::Ref(a) | Cell::Str(a) => {
                if let ast::Term::Var(_) = term {
                    let mut buffer = String::new();
                    resolve_term(&m, *a, &display_vec, &mut buffer);

                    if buffer != term.to_string() {
                        let program_binding = format!("{} = {}", term, buffer);

                        if !query_bindings.contains(&program_binding) {
                            program_bindings.push(format!("{} = {}", term, buffer));
                        }

                    }
                }
            },
            _ => ()
        }
    }

    query_bindings.sort();
    program_bindings.sort();

    (query_bindings, program_bindings)
}

pub fn program(m: &mut Machine, p: &str) -> HashMap<Cell, Term> {
    let e = parser::ExpressionParser::new();
    let mut program = e.parse(p).unwrap();
    let mut map = HashMap::new();

    if let t@Term::Compound(_) | t@Term::Atom(_) = &mut program {
        let mut seen = HashSet::new();
        let instructions = compile_fact(t, &mut map, &mut seen);
        let mut output = HashMap::new();

        {
            let compound = t.structuralize().unwrap();
            let name = compound.name;
            let arity = compound.arity;
            let program_functor = Functor(name, arity);
            m.push_instructions(&program_functor, &instructions);
            m.execute_instructions(&program_functor);
        }

        for (term, x) in &map {
            output.insert(m.get_register(*x).cloned().unwrap(), term.clone());
        }

        output
    } else {
        panic!("not supported yet")
    }
}

pub fn resolve_term(m: &Machine, addr: HeapAddress, display_map: &[(Cell, Term)], term_string: &mut String) {
    let d = m.deref(Store::HeapAddr(addr));
    let cell = m.get_store_cell(d);


    match cell {
        Cell::Func(Functor(name, arity)) => {
            if *arity == 0 {
                term_string.push_str(name);
            } else {
                term_string.push_str(&format!("{}(", name));
            }

            for i in 1..=*arity {
                resolve_term(&m, d.address() + i, display_map, term_string);

                if i != *arity {
                    term_string.push_str(", ");
                }
            }

            if *arity > 0 {
                term_string.push_str(")");
            }
        },
        Cell::Str(a) => {
            resolve_term(&m, *a, display_map, term_string)
        },
        Cell::Ref(r) => {
            if *r == d.address() {
                for (cell, term) in display_map {
                    if let Ref(a) = cell {
                        if *a == *r {
                            if let Term::Var(_) = term {
                                let s = format!("{}", term);

                                term_string.push_str(&s);
                                break;
                            }
                        }
                    }
                }
            } else {
                resolve_term(&m, *r, display_map, term_string);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_logger() {
        env_logger::builder()
            .is_test(true)
            .default_format_timestamp(false)
            .try_init()
            .unwrap()
    }

    #[test]
    fn test_set_variable() {
        let mut m = Machine::new();

        m.set_variable(X(1));

        let expected_heap_cells = vec![Ref(0)];
        let heap_cells = &m.heap;

        assert_eq!(heap_cells, &expected_heap_cells);
        register_is(&m, X(1), Ref(0));
    }

    #[test]
    fn test_set_value() {
        let mut m = Machine::new();

        m.set_variable(X(1));
        m.set_variable(X(2));

        m.set_value(X(1));
        m.set_value(X(2));

        let expected_heap_cells = vec![Ref(0), Ref(1), Ref(0), Ref(1)];
        let heap_cells = &m.heap;

        assert_eq!(heap_cells, &expected_heap_cells);
        register_is(&m, X(1), Ref(0));
        register_is(&m, X(2), Ref(1));
        assert_eq!(m.registers.x.len(), 2);
    }

    #[test]
    fn test_put_structure() {
        let mut m = Machine::new();

        m.put_structure(Functor(String::from("foo"), 2), X(1));
        m.set_variable(X(2));
        m.set_variable(X(3));
        m.set_value(X(2));

        let expected_heap_cells = vec![
            Str(1),
            Cell::from("foo/2"),
            Ref(2),
            Ref(3),
            Ref(2)
        ];

        let heap_cells = &m.heap;

        println!("{:?}", m.get_x_registers());
        assert_eq!(heap_cells, &expected_heap_cells);
        register_is(&m, X(1), Str(1));
        register_is(&m, X(2), Ref(2));
        register_is(&m, X(3), Ref(3));
        assert_eq!(m.registers.x.len(), 3);
    }

    #[test]
    fn test_deref() {
        let mut m = Machine::new();

        m.heap = vec![
            Ref(2),
            Ref(3),
            Ref(1),
            Ref(3),
            Str(5),
            Cell::from("f/2"),
            Ref(3)
        ];

        m.insert_x(3, Ref(4));

        assert_eq!(m.deref(HeapAddr(0)), HeapAddr(3));
        assert_eq!(m.deref(HeapAddr(1)), HeapAddr(3));
        assert_eq!(m.deref(HeapAddr(2)), HeapAddr(3));
        assert_eq!(m.deref(HeapAddr(3)), HeapAddr(3));
        assert_eq!(m.deref(HeapAddr(4)), HeapAddr(4));
        assert_eq!(m.deref(HeapAddr(5)), HeapAddr(5));
        assert_eq!(m.deref(HeapAddr(6)), HeapAddr(3));
        assert_eq!(m.deref(Register(X(3))), HeapAddr(4));
    }

    #[test]
    fn test_exercise_2_1() {
        let mut m = Machine::new();

        m.put_structure(Functor::from("h/2"), X(3));
        m.set_variable(X(2));
        m.set_variable(X(5));
        m.put_structure(Functor::from("f/1"), X(4));
        m.set_value(X(5));
        m.put_structure(Functor::from("p/3"), X(1));
        m.set_value(X(2));
        m.set_value(X(3));
        m.set_value(X(4));


        let expected_heap_cells = vec![
            Str(1),
            Cell::from("h/2"),
            Ref(2),
            Ref(3),
            Str(5),
            Cell::from("f/1"),
            Ref(3),
            Str(8),
            Cell::from("p/3"),
            Ref(2),
            Str(1),
            Str(5),
        ];

        let heap_cells = &m.heap;
        assert_eq!(heap_cells, &expected_heap_cells);

        register_is(&m, X(1), Str(8));
        register_is(&m, X(2), Ref(2));
        register_is(&m, X(3), Str(1));
        register_is(&m, X(4), Str(5));
        register_is(&m, X(5), Ref(3));
    }

    #[test]
    fn test_exercise_2_3() {
        let mut m = Machine::new();

        m.put_structure(Functor::from("h/2"), X(3));
        m.set_variable(X(2));
        m.set_variable(X(5));
        m.put_structure(Functor::from("f/1"), X(4));
        m.set_value(X(5));
        m.put_structure(Functor::from("p/3"), X(1));
        m.set_value(X(2));
        m.set_value(X(3));
        m.set_value(X(4));

        m.get_structure(Functor::from("p/3"), X(1));
        m.unify_variable(X(2));
        m.unify_variable(X(3));
        m.unify_variable(X(4));
        m.get_structure(Functor::from("f/1"), X(2));
        m.unify_variable(X(5));
        m.get_structure(Functor::from("h/2"), X(3));
        m.unify_value(X(4));
        m.unify_variable(X(6));
        m.get_structure(Functor::from("f/1"), X(6));
        m.unify_variable(X(7));
        m.get_structure(Functor::from("a/0"), X(7));

        let expected_heap_cells = vec![
            Str(1),
            Cell::from("h/2"),
            Str(13),
            Str(16),
            Str(5),
            Cell::from("f/1"),
            Ref(3),
            Str(8),
            Cell::from("p/3"),
            Ref(2),
            Str(1),
            Str(5),
            Str(13),
            Cell::from("f/1"),
            Ref(3),
            Str(16),
            Cell::from("f/1"),
            Str(19),
            Str(19),
            Cell::from("a/0")
        ];


        let heap_cells = &m.heap;
        assert_eq!(heap_cells, &expected_heap_cells);


        register_is(&m, X(1), Str(8));
        register_is(&m, X(2), Ref(2));
        register_is(&m, X(3), Str(1));
        register_is(&m, X(4), Str(5));
        register_is(&m, X(5), Ref(14));
        register_is(&m, X(6), Ref(3));
        register_is(&m, X(7), Ref(17));
    }

    #[test]
    fn test_program_instruction_compilation_fig_2_3() {
        let e = parser::ExpressionParser::new();

        let expected_instructions = vec![
            Instruction::PutVariable(X(4), X(1)),
            Instruction::PutStructure(Functor::from("h/2"), X(2)),
            Instruction::SetValue(X(4)),
            Instruction::SetVariable(X(5)),
            Instruction::PutStructure(Functor::from("f/1"), X(3)),
            Instruction::SetValue(X(5)),
            Instruction::Call(Functor::from("p/3"))
        ];

        let mut p = e.parse("p(Z, h(Z, W), f(W)).").unwrap();
        let mut map = HashMap::new();
        let mut seen = HashSet::new();
        let instructions = compile_query(&p, &mut map, &mut seen);

        assert_eq!(expected_instructions, instructions);
    }

    #[test]
    fn test_program_instruction_compilation_fig_2_4() {
        let e = parser::ExpressionParser::new();

        let expected_instructions = vec![
            Instruction::GetStructure(Functor::from("f/1"), X(1)),
            Instruction::UnifyVariable(X(4)),
            Instruction::GetStructure(Functor::from("h/2"), X(2)),
            Instruction::UnifyVariable(X(5)),
            Instruction::UnifyVariable(X(6)),
            Instruction::GetValue(X(5), X(3)),
            Instruction::GetStructure(Functor::from("f/1"), X(6)),
            Instruction::UnifyVariable(X(7)),
            Instruction::GetStructure(Functor::from("a/0"), X(7)),
            Instruction::Proceed
        ];

        let mut p = e.parse("p(f(X), h(Y, f(a)), Y).").unwrap();
        let mut seen = HashSet::new();
        let mut map = HashMap::new();
        let instructions = compile_fact(&p, &mut map, &mut seen);

        assert_eq!(expected_instructions, instructions);
    }

    #[test]
    fn test_instruction_compilation_exercise_2_4() {
        let e = parser::ExpressionParser::new();

        let expected_query_instructions = vec![
            Instruction::PutStructure(Functor::from("f/1"), X(1)),
            Instruction::SetVariable(X(4)),
            Instruction::PutStructure(Functor::from("a/0"), X(7)),
            Instruction::PutStructure(Functor::from("f/1"), X(6)),
            Instruction::SetValue(X(7)),
            Instruction::PutStructure(Functor::from("h/2"), X(2)),
            Instruction::SetVariable(X(5)),
            Instruction::SetValue(X(6)),
            Instruction::PutValue(X(5), X(3)),
            Instruction::Call(Functor::from("p/3"))
        ];

        let expected_program_instructions = vec![
            Instruction::GetVariable(X(4), X(1)),
            Instruction::GetStructure(Functor::from("h/2"), X(2)),
            Instruction::UnifyValue(X(4)),
            Instruction::UnifyVariable(X(5)),
            Instruction::GetStructure(Functor::from("f/1"), X(3)),
            Instruction::UnifyValue(X(5)),
            Instruction::Proceed
        ];

        let mut q = e.parse("p(f(X), h(Y, f(a)), Y).").unwrap();
        let mut p = e.parse("p(Z, h(Z, W), f(W)).").unwrap();
        let mut query_seen = HashSet::new();
        let mut query_map = HashMap::new();
        let mut program_seen = HashSet::new();
        let mut program_map = HashMap::new();

        let query_instructions = compile_query(&q, &mut query_map, &mut query_seen);
        let program_instructions = compile_fact(&p, &mut program_map, &mut program_seen);

        assert_eq!(expected_query_instructions, query_instructions);
        assert_eq!(expected_program_instructions, program_instructions);
    }

    #[test]
    fn test_query_execution_2_6() {
        let mut m = Machine::new();


        m.put_variable(X(4), X(1));
        m.put_structure(Functor::from("h/2"), X(2));
        m.set_value(X(4));
        m.set_variable(X(5));
        m.put_structure(Functor::from("f/1"), X(3));
        m.set_value(X(5));
//        m.call(Functor::from("p/3"));

        let expected_heap_cells = vec![
            Ref(0),
            Str(2),
            Cell::from("h/2"),
            Ref(0),
            Ref(4),
            Str(6),
            Cell::from("f/1"),
            Ref(4)
        ];

        let heap_cells = m.get_heap();
        assert_eq!(heap_cells, &expected_heap_cells);
    }

    #[test]
    fn test_exercise_2_7() {
        let mut m = Machine::new();

        m.put_variable(X(4), X(1));
        m.put_structure(Functor::from("h/2"), X(2));
        m.set_value(X(4));
        m.set_variable(X(5));
        m.put_structure(Functor::from("f/1"), X(3));
        m.set_value(X(5));
//        m.call(Functor::from("p/3"));

        m.get_structure(Functor::from("f/1"), X(1));
        m.unify_variable(X(4));
        m.get_structure(Functor::from("h/2"), X(2));
        m.unify_variable(X(5));
        m.unify_variable(X(6));
        m.get_value(X(5), X(3));
        m.get_structure(Functor::from("f/1"), X(6));
        m.unify_variable(X(7));
        m.get_structure(Functor::from("a/0"), X(7));
        m.proceed();

        let expected_heap_cells = vec![
            Str(9),
            Str(2),
            Cell::from("h/2"),
            Ref(0),
            Str(12),
            Str(6),
            Cell::from("f/1"),
            Ref(4),
            Str(9),
            Cell::from("f/1"),
            Ref(4),
            Str(12),
            Cell::from("f/1"),
            Str(15),
            Str(15),
            Cell::from("a/0")
        ];


        let heap_cells = &m.heap;
        assert_eq!(heap_cells, &expected_heap_cells);


        register_is(&m, X(1), Ref(0));
        register_is(&m, X(2), Str(2));
        register_is(&m, X(3), Str(6));
        register_is(&m, X(4), Ref(10));
        register_is(&m, X(5), Ref(0));
        register_is(&m, X(6), Ref(4));
        register_is(&m, X(7), Ref(13));
    }

    #[test]
    fn test_instruction_compilation_exercise_2_8() {
        let q = Compound {
            name: "p".to_string(),
            arity: 3,
            args: vec![
                Term::Compound(Compound { name: "f".to_string(), arity: 1, args: vec![Term::Var(Var("X".to_string()))] }),
                Term::Compound(
                    Compound {
                        name: "h".to_string(),
                        arity: 2,
                        args: vec![
                            Term::Var(Var("Y".to_string())),
                            Term::Compound(Compound {
                                name: "f".to_string(),
                                arity: 1,
                                args: vec![Term::Atom(Atom("a".to_string()))]
                            })
                        ]
                    }
                ),
                Term::Var(Var("Y".to_string()))
            ]
        };

        let p = Compound {
            name: "p".to_string(),
            arity: 3,
            args: vec![
                Term::Var(Var("Z".to_string())),
                Term::Compound(
                    Compound {
                        name: "h".to_string(),
                        arity: 2,
                        args: vec![
                            Term::Var(Var("Z".to_string())),
                            Term::Var(Var("W".to_string()))
                        ]
                    }
                ),
                Term::Compound(
                    Compound {
                        name: "f".to_string(),
                        arity: 1,
                        args: vec![
                            Term::Var(Var("W".to_string()))
                        ]
                    }
                )
            ]
        };

        let expected_query_instructions = vec![
            Instruction::PutStructure(Functor::from("f/1"), X(1)),
            Instruction::SetVariable(X(4)),
            Instruction::PutStructure(Functor::from("a/0"), X(7)),
            Instruction::PutStructure(Functor::from("f/1"), X(6)),
            Instruction::SetValue(X(7)),
            Instruction::PutStructure(Functor::from("h/2"), X(2)),
            Instruction::SetVariable(X(5)),
            Instruction::SetValue(X(6)),
            Instruction::PutValue(X(5), X(3)),
            Instruction::Call(Functor::from("p/3"))
        ];

        let expected_program_instructions = vec![
            Instruction::GetVariable(X(4), X(1)),
            Instruction::GetStructure(Functor::from("h/2"), X(2)),
            Instruction::UnifyValue(X(4)),
            Instruction::UnifyVariable(X(5)),
            Instruction::GetStructure(Functor::from("f/1"), X(3)),
            Instruction::UnifyValue(X(5)),
            Instruction::Proceed
        ];

        let mut query_seen = HashSet::new();
        let mut query_map = HashMap::new();
        let mut program_seen = HashSet::new();
        let mut program_map = HashMap::new();

        let query_instructions = compile_query(&q, &mut query_map, &mut query_seen);
        let program_instructions = compile_fact(&p, &mut program_map, &mut program_seen);

        assert_eq!(&expected_query_instructions, &query_instructions);
        assert_eq!(&expected_program_instructions, &program_instructions);
    }

    #[test]
    fn test_instruction_compilation_figure_2_9() {
        let q = Compound {
            name: "p".to_string(),
            arity: 3,
            args: vec![
                Term::Var(Var("Z".to_string())),
                Term::Compound(
                    Compound {
                        name: "h".to_string(),
                        arity: 2,
                        args: vec![
                            Term::Var(Var("Z".to_string())),
                            Term::Var(Var("W".to_string()))
                        ]
                    }
                ),
                Term::Compound(
                    Compound {
                        name: "f".to_string(),
                        arity: 1,
                        args: vec![
                            Term::Var(Var("W".to_string()))
                        ]
                    }
                )
            ]
        };

        let expected_query_instructions = vec![
            Instruction::PutVariable(X(4), X(1)),
            Instruction::PutStructure(Functor::from("h/2"), X(2)),
            Instruction::SetValue(X(4)),
            Instruction::SetVariable(X(5)),
            Instruction::PutStructure(Functor::from("f/1"), X(3)),
            Instruction::SetValue(X(5)),
            Instruction::Call(Functor::from("p/3"))
        ];

        let mut seen = HashSet::new();
        let mut map = HashMap::new();
        let query_instructions = compile_query(&q, &mut map, &mut seen);

        assert_eq!(&expected_query_instructions, &query_instructions);
    }

    #[test]
    fn test_instruction_compilation_figure_2_10() {
        let q = Compound {
            name: "p".to_string(),
            arity: 3,
            args: vec![
                Term::Compound(
                    Compound {
                        name: "f".to_string(),
                        arity: 1,
                        args: vec![Term::Var(Var("X".to_string()))]
                    }
                ),
                Term::Compound(
                    Compound {
                        name: "h".to_string(),
                        arity: 2,
                        args: vec![
                            Term::Var(Var("Y".to_string())),
                            Term::Compound(
                                Compound {
                                    name: "f".to_string(),
                                    arity: 1,
                                    args: vec![Term::Atom(Atom("a".to_string()))]
                                }
                            ),
                        ]
                    }
                ),
                Term::Var(Var("Y".to_string()))
            ]
        };

        let expected_program_instructions = vec![
            Instruction::GetStructure(Functor::from("f/1"), X(1)),
            Instruction::UnifyVariable(X(4)),
            Instruction::GetStructure(Functor::from("h/2"), X(2)),
            Instruction::UnifyVariable(X(5)),
            Instruction::UnifyVariable(X(6)),
            Instruction::GetValue(X(5), X(3)),
            Instruction::GetStructure(Functor::from("f/1"), X(6)),
            Instruction::UnifyVariable(X(7)),
            Instruction::GetStructure(Functor::from("a/0"), X(7)),
            Instruction::Proceed
        ];

        let mut seen = HashSet::new();
        let mut map = HashMap::new();
        let program_instructions = compile_fact(&q, &mut map, &mut seen);

        assert_eq!(&expected_program_instructions, &program_instructions);
    }

    #[test]
    fn test_fact_instruction_compilation_exercise_3_1() {
        let fact1 = Term::Compound(Compound {
            name: "q".to_string(),
            arity: 2,
            args: vec![
                Term::Atom(Atom("a".to_string())),
                Term::Atom(Atom("b".to_string()))
            ]});

        let fact2 = Term::Compound(Compound {
            name: "r".to_string(),
            arity: 2,
            args: vec![
                Term::Atom(Atom("b".to_string())),
                Term::Atom(Atom("c".to_string()))
            ]});

        let r = Rule {
            head: Compound {
                name: "p".to_string(),
                arity: 2,
                args: vec![
                    Term::Var(Var("X".to_string())),
                    Term::Var(Var("Y".to_string()))
                ]},
            body: vec![
                Compound {
                    name: "q".to_string(),
                    arity: 2,
                    args: vec![
                        Term::Var(Var("X".to_string())),
                        Term::Var(Var("Z".to_string()))
                    ]},
                Compound {
                    name: "r".to_string(),
                    arity: 2,
                    args: vec![
                        Term::Var(Var("Z".to_string())),
                        Term::Var(Var("Y".to_string()))
                    ]},
            ]
        };

        let expected_fact1_instructions = vec![
            Instruction::GetStructure(Functor::from("a/0"), X(1)),
            Instruction::GetStructure(Functor::from("b/0"), X(2)),
            Instruction::Proceed
        ];

        let expected_fact2_instructions = vec![
            Instruction::GetStructure(Functor::from("b/0"), X(1)),
            Instruction::GetStructure(Functor::from("c/0"), X(2)),
            Instruction::Proceed
        ];

        let expected_rule_instructions = vec![
            Instruction::Allocate(2),
            Instruction::GetVariable(X(3), X(1)),
            Instruction::GetVariable(Y(1), X(2)),
            Instruction::PutValue(X(3), X(1)),
            Instruction::PutVariable(Y(2), X(2)),
            Instruction::Call(Functor::from("q/2")),
            Instruction::PutValue(Y(2), X(1)),
            Instruction::PutValue(Y(1), X(2)),
            Instruction::Call(Functor::from("r/2")),
            Instruction::Deallocate
        ];

        let mut m = HashMap::new();
        let mut seen= HashSet::new();

        let rule_instructions = compile_rule(&r, &mut m, &mut seen);
        let fact1_instructions = compile_fact(&fact1, &mut m, &mut seen);
        let fact2_instructions = compile_fact(&fact2, &mut m, &mut seen);
        let r_functor = Functor::from("p/2");
        let f1_functor = Functor::from("q/2");
        let f2_functor = Functor::from("r/2");

        assert_eq!(&expected_fact1_instructions, &fact1_instructions);
        assert_eq!(&expected_fact2_instructions, &fact2_instructions);
        assert_eq!(&expected_rule_instructions, &rule_instructions);

        let mut machine = Machine::new();


        machine.push_instructions(&r_functor, &rule_instructions);
        machine.push_instructions(&f1_functor, &fact1_instructions);
        machine.push_instructions(&f2_functor, &fact2_instructions);
        let results = query(&mut machine, "p(U, V).");
        machine.execute_instructions(&r_functor);
        machine.execute_instructions(&f1_functor);
        machine.execute_instructions(&f2_functor);

        println!("{:?}", machine);
        println!("{:?}", results);
    }

    #[test]
    fn test_unify_variable_read_mode() {
        let mut m = Machine::new();

        m.set_mode(Read);
        m.push_heap(Ref(3));
        m.unify_variable(X(1));

        assert_eq!(m.get_x(1).cloned().unwrap(), Ref(3));
        assert_eq!(m.get_s(), 1);
    }

    #[test]
    fn test_unify_variable_write_mode() {
        let mut m = Machine::new();

        m.set_mode(Write);
        m.unify_variable(X(1));

        assert_eq!(m.heap[0], Ref(0));
        assert_eq!(m.get_x(1).cloned().unwrap(), Ref(0));
        assert_eq!(m.get_h(), 1);
        assert_eq!(m.get_s(), 1);
    }

    #[test]
    fn test_functor_eq() {
        let f1 = Functor::from("foo/1");
        let f2 = Functor::from("bar/1");

        assert_ne!(f1, f2);

        let f2 = Functor::from("foo/1");
        assert_eq!(f1, f2);

        let f2 = Functor::from("foo/2");
        assert_ne!(f1, f2);
    }

    #[test]
    fn test_compound_structure_rendering() {
        let t = Term::Compound( Compound {
            name: String::from("foo"),
            arity: 2,
            args: vec![Term::Atom(Atom("bar".to_string())), Term::Atom(Atom("baz".to_string()))]});

        assert_eq!(t.to_string(), "foo(bar, baz)");
    }

    #[test]
    fn test_atomic_structure_rendering() {
        let t = Term::Compound( Compound { name: String::from("bar"), arity: 0, args: Vec::new() });

        assert_eq!(t.to_string(), "bar");
    }

    #[test]
    fn test_atom_parser() {
        let atom_parser = parser::AtomParser::new();

        // atoms
        assert!(atom_parser.parse("22").is_err());
        assert!(atom_parser.parse("_Abc").is_err());
        assert!(atom_parser.parse("Abc").is_err());
        assert!(atom_parser.parse("abc").is_ok());
        assert!(atom_parser.parse("'Abc'").is_ok());
        assert!(atom_parser.parse("'Abc").is_err());
        assert!(atom_parser.parse(".q").is_err());
        assert!(atom_parser.parse("snake_case").is_ok());
        assert!(atom_parser.parse("'snake_case'").is_ok());
        assert!(atom_parser.parse("This_Fails").is_err());
        assert!(atom_parser.parse("'This_Succeeds'").is_ok());
    }

    #[test]
    fn test_number_parser() {
        let number_parser = parser::NumberParser::new();

        // numbers
        assert!(number_parser.parse("2").is_ok());
        assert!(number_parser.parse("42").is_ok());
        assert!(number_parser.parse("34345354").is_ok());
//    assert!(number_parser.parse("3.3").is_ok());
//    assert!(number_parser.parse("3.30").is_ok());
//    assert!(number_parser.parse("0.3").is_ok());
        assert!(number_parser.parse("a03").is_err());
        assert!(number_parser.parse("_21").is_err());
        assert!(number_parser.parse("2_12").is_err());
        assert!(number_parser.parse(".3").is_err());
        assert!(number_parser.parse("2.").is_err());
    }

    #[test]
    fn test_compound_parser() {
        let c = parser::CompoundParser::new();

        // compounds
        assert!(c.parse("p(Z, h(Z, W), f(W))").is_ok());
        assert!(c.parse("p (Z, h(Z, W), f(W))").is_err());
        assert!(c.parse("p(Z, h(Z, W), f(W)").is_err());
        assert!(c.parse("p(Z, h(Z,, f(W)").is_err());
        assert!(c.parse("p(Z, f(h(Z, W)), f(W))").is_ok());
    }

    #[test]
    fn test_simple_expressions() {
        let e = parser::ExpressionParser::new();

        //expressions
        assert!(e.parse("A.").is_ok());
        assert!(e.parse("2.").is_err());
        assert!(e.parse("foo(bar).").is_ok());
        assert!(e.parse("foo.").is_ok());
    }

    #[test]
    fn test_instruction_compilation_exercise_2_1() {
        let e = parser::ExpressionParser::new();

        let expected_instructions = vec![
            Instruction::PutVariable(X(4), X(1)),
            Instruction::PutStructure(Functor::from("h/2"), X(2)),
            Instruction::SetValue(X(4)),
            Instruction::SetVariable(X(5)),
            Instruction::PutStructure(Functor::from("f/1"), X(3)),
            Instruction::SetValue(X(5)),
            Instruction::Call(Functor::from("p/3"))
        ];

        let mut q = e.parse("p(Z, h(Z, W), f(W)).").unwrap();
        let mut seen = HashSet::new();
        let mut map = HashMap::new();
        let instructions = compile_query(&q, &mut map, &mut seen);

        assert_eq!(expected_instructions, instructions);
    }

    fn register_is(machine: &Machine, register: Register, cell: Cell) {
        assert_eq!(machine.get_register(register).cloned().unwrap(), cell);
    }
}
