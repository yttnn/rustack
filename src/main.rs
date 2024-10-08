use core::num;
use std::{cmp::Ordering, collections::HashMap, io::{BufReader, BufWriter, Read, Write}, ops::{Add, ControlFlow}};

use nom::{
  branch::alt, bytes::complete::tag, character::complete::{alpha1, alphanumeric1, char, multispace0, multispace1, none_of}, combinator::{cut, map_res, opt, recognize}, error::ParseError, multi::{fold_many0, many0, separated_list0}, number::complete::recognize_float, sequence::{delimited, pair, preceded, terminated}, Finish, IResult, InputTake, Offset,Parser
};
use nom_locate::LocatedSpan;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum OpCode {
  LoadLiteral,
  Copy,
  Add,
}

macro_rules! impl_op_from {
  ($($op:ident),*) => {
    impl From<u8> for OpCode {
      #[allow(non_upper_case_globals)]
      fn from(o: u8) -> Self {
        $(const $op: u8 = OpCode::$op as u8;)*

        match o {
          $($op => Self::$op,)*
          _ => panic!("Opcode \"{:02X}\" unrecognized!", o),
        }
      }
    }
  }
}

impl_op_from!(LoadLiteral, Copy, Add);

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct Instruction {
  op: OpCode,
  arg0: u8,
}

impl Instruction {
  fn new(op: OpCode, arg0: u8) -> Self {
    Self { op, arg0 }
  }

  fn serialize(
    &self,
    writer: &mut impl Write,
  ) -> Result<(), std::io::Error> {
    println!("serialize: {:?}", &[self.op as u8, self.arg0]);
    writer.write_all(&[self.op as u8, self.arg0])?;
    Ok(())
  }

  fn deserialize(
    reader: &mut impl Read,
  ) -> Result<Self, std::io::Error> {
    let mut buf = [0u8; 2];
    reader.read_exact(&mut buf)?;
    Ok(Self::new(buf[0].into(), buf[1]))
  }
}

fn serialize_size(
  sz: usize,
  writer: &mut impl Write,
) -> std::io::Result<()> {
  println!("size: {:?}", &(sz as u32).to_le_bytes());
  writer.write_all(&(sz as u32).to_le_bytes())
}

fn deserialize_size(
  reader: &mut impl Read,
) -> std::io::Result<usize> {
  let mut buf = [0u8; std::mem::size_of::<u32>()];
  reader.read_exact(&mut buf)?;
  Ok(u32::from_le_bytes(buf) as usize)
}

struct Compiler {
  literals: Vec<f64>,
  instructions: Vec<Instruction>,
  target_stack: Vec<usize>,
}

impl Compiler {
  fn new() -> Self {
    Self {
      literals: vec![],
      instructions: vec![],
      target_stack: vec![],
    }
  }

  fn add_literal(&mut self, value: f64) -> u8 {
    let ret = self.literals.len();
    self.literals.push(value);
    ret as u8
  }

  fn add_inst(&mut self, op: OpCode, arg0: u8) -> usize {
    let inst = self.instructions.len();
    self.instructions.push(Instruction { op, arg0 });
    inst
  }

  fn add_copy_inst(&mut self, stack_idx: usize) -> usize {
    let inst = self.add_inst(OpCode::Copy, (self.target_stack.len() - stack_idx - 1) as u8);
    self.target_stack.push(0);
    inst
  }

  fn write_literals(
    &self,
    writer: &mut impl Write,
  ) -> std::io::Result<()> {
    serialize_size(self.literals.len(), writer)?;
    for value in &self.literals {
      // println!("literal: {:?}", &value.to_le_bytes());
      writer.write_all(&value.to_le_bytes())?;
    }
    Ok(())
  }

  fn write_insts(
    &self,
    writer: &mut impl Write,
  ) -> std::io::Result<()> {
    serialize_size(self.instructions.len(), writer)?;
    for instruction in &self.instructions {
      instruction.serialize(writer).unwrap();
    }
    Ok(())
  }

  fn compile_expr(&mut self, ex: &Expression) -> usize {
    match ex {
      Expression::NumLiteral(num) => {
        let id = self.add_literal(*num);
        self.add_inst(OpCode::LoadLiteral, id);
        self.target_stack.push(id as usize);
        self.target_stack.len() - 1
      }
      Expression::Ident("pi") => {
        let id = self.add_literal(std::f64::consts::PI as f64);
        self.add_inst(OpCode::LoadLiteral, id);
        self.target_stack.push(id as usize);
        self.target_stack.len() - 1
      }
      Expression::Ident(id) => {
        panic!("Unknown identifier {id:?}");
      }
      Expression::Add(lhs, rhs) => {
        let lhs = self.compile_expr(lhs);
        let rhs = self.compile_expr(rhs);
        self.add_copy_inst(lhs);
        self.add_copy_inst(rhs);
        self.target_stack.pop();
        self.add_inst(OpCode::Add, 0);
        self.target_stack.len() - 1
      }
    }
  }

  fn diasm(
    &self,
    writer: &mut impl Write,
  ) -> std::io::Result<()> {
    use OpCode::*;
    writeln!(writer, "Literals [{}]", self.literals.len())?;
    for (i, con) in self.literals.iter().enumerate() {
      writeln!(writer, "  [{i}] {}", *con);
    }

    writeln!(
      writer,
      "Instructions [{}]",
      self.instructions.len()
    )?;
    for (i, inst) in self.instructions.iter().enumerate() {
      match inst.op {
        LoadLiteral => writeln!(
          writer,
          "  [{i}] {:?} {} ({:?})",
          inst.op, inst.arg0, self.literals[inst.arg0 as usize]
        )?,
        Copy => writeln!(
          writer,
          "  [{i}] {:?} {}",
          inst.op, inst.arg0
        )?,
        Add => writeln!(writer, "  [{i}] Add")?,
      }
    }
    Ok(())
  }
}

fn write_program(
  source: &str,
  writer: &mut impl Write,
  out_file: &str,
  disasm: bool,
) -> std::io::Result<()> {
  let mut compiler = Compiler::new();
  let (_, ex) = expr(source).map_err(|e| {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_owned())
  })?;

  compiler.compile_expr(&ex);

  if disasm {
    compiler.diasm(&mut std::io::stdout())?;
  }

  compiler.write_literals(writer).unwrap();
  compiler.write_insts(writer).unwrap();
  println!(
    "Written {} literals and {} instructions to {out_file:?}",
    compiler.literals.len(),
    compiler.instructions.len()
  );
  Ok(())
}

struct ByteCode {
  literals: Vec<f64>,
  instructions: Vec<Instruction>,
}

impl ByteCode {
  fn new() -> Self {
    Self {
      literals: vec![],
      instructions: vec![],
    }
  }

  fn read_literals(
    &mut self,
    reader: &mut impl Read,
  ) -> std::io::Result<()> {
    let num_literals = deserialize_size(reader)?;
    for _ in 0..num_literals {
      let mut buf = [0u8; std::mem::size_of::<i64>()];
      reader.read_exact(&mut buf)?;
      self.literals.push(f64::from_le_bytes(buf));
    }
    Ok(())
  }

  fn read_instructions(
    &mut self,
    reader: &mut impl Read,
  ) -> std::io::Result<()> {
    let num_instructions = deserialize_size(reader)?;
    for _ in 0..num_instructions {
      let inst = Instruction::deserialize(reader)?;
      self.instructions.push(inst);
    }
    Ok(())
  }

  fn interpret(&self) -> Option<f64> {
    let mut stack = vec![];

    for (ip, instruction) in self.instructions.iter().enumerate() {
      println!("interpret[{ip}]: {instruction:?} stack: {stack:?}");
      match instruction.op {
        OpCode::LoadLiteral => {
          stack.push(self.literals[instruction.arg0 as usize]);
        }
        OpCode::Copy => {
          stack.push(stack[stack.len() - instruction.arg0 as usize - 1]);
        }
        OpCode::Add => {
          let rhs = stack.pop().expect("Stack underflow");
          let lhs = stack.pop().expect("Stack underflow");
          stack.push(lhs + rhs);
        }
      }
    }
    stack.pop()
  }
}

fn read_program(
  reader: &mut impl Read,
) -> std::io::Result<ByteCode> {
  let mut bytecode = ByteCode::new();
  bytecode.read_literals(reader)?;
  bytecode.read_instructions(reader)?;
  Ok(bytecode)
}

fn main() {

  let mut args = std::env::args();
  args.next();
  match args.next().as_ref().map(|s| s as &str) {
    Some("w") => write_program("bytecode.bin").unwrap(),
    Some("r") => {
      if let Ok(bytecode) = read_program("bytecode.bin") {
        let result = bytecode.interpret();
        println!("result: {result:?}");
      }
    }
    _ => println!("Please specify w or r as an arugument"),
  }

  // let mut buf = String::new();
  // if !std::io::stdin().read_to_string(&mut buf).is_ok() {
  //   panic!("Failed to read from stdin");
  // }
  // let parsed_statements = match statements_finish(Span::new(&buf)) {
  //   Ok(parsed_statements) => parsed_statements,
  //   Err(e) => {
  //     eprintln!("Parse error: {e:?}");
  //     return;
  //   }
  // };

  // let mut tc_ctx = TypeCheckContext::new();

  // if let Err(err) = type_check(&parsed_statements, &mut tc_ctx) {
  //   println!("Type check error: {err}");
  //   return;
  // }
  // println!("Type check OK");

  // let mut frame = StackFrame::new();

  // eval_stmts(&parsed_statements, &mut frame);
}

#[derive(Debug, PartialEq, Clone)]
enum Expression<'src> {
  Ident(&'src str),
  NumLiteral(f64),
  Add(Box<Expression<'src>>, Box<Expression<'src>>),
}

fn term(i: &str) -> IResult<&str, Expression> {
  alt((number, ident, parens))(i)
}

fn ident(input: &str) -> IResult<&str, Expression> {
  let (r, res) =
    delimited(multispace0, identifier, multispace0)(input)?;
  Ok((r, Expression::Ident(res)))
}

fn identifier(input: &str) -> IResult<&str, &str> {
  recognize(pair(
    alt((alpha1, tag("_"))),
    many0(alt((alphanumeric1, tag("_")))),
  ))(input)
}

fn number(input: &str) -> IResult<&str, Expression> {
  let (r, v) =
    delimited(multispace0, recognize_float, multispace0)(
      input,
    )?;
  Ok((
    r,
    Expression::NumLiteral(v.parse().map_err(|_| {
      nom::Err::Error(nom::error::Error {
        input,
        code: nom::error::ErrorKind::Digit,
      })
    })?),
  ))
}

fn parens(i: &str) -> IResult<&str, Expression> {
  delimited(
    multispace0,
    delimited(tag("("), expr, tag(")")),
    multispace0,
  )(i)
}

fn expr(i: &str) -> IResult<&str, Expression> {
  let (i, init) = term(i)?;

  fold_many0(
    pair(delimited(multispace0, char('+'), multispace0), term),
    move || init.clone(),
    |acc, (_op, val): (char, Expression)| {
      Expression::Add(Box::new(acc), Box::new(val))
    },
  )(i)
}