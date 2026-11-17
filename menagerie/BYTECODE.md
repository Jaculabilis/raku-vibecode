# Menagerie bytecode

One bytecode serves Lox, Monkey and Tiger. It is a dynamically typed stack
machine with first-class closures. Every language-specific behaviour is an
explicit instruction: for example, Tiger's `if` tests `JumpIfZero` while Lox
and Monkey test `JumpIfFalse`. No instruction looks at which language
produced it. The VM uses the source language of each function for one thing
only: phrasing runtime error messages the way that language's reference
implementation would.

The implementation lives in `src/bytecode.rs` (format, linking,
verification, disassembly) and `src/vm.rs` (interpreter).

## Machine model

* A **program** is a constant pool, a table of function **prototypes** and
  a list of **modules**. Running a program runs each module's entry
  function in order. All modules share one **global namespace**, a map
  from names to values. That namespace is how code in different languages
  finds each other.
* Each call gets a **frame**: `nlocals` local slots, followed by an operand
  stack. The caller puts the arguments into the first slots, and the VM
  fills the remaining slots with the internal value *undefined*.
* A **closure** pairs a prototype with an array of captured **cells**. A
  local variable that some inner function captures lives in a heap cell
  stored in its slot (`InitCell`, `GetCell`, `SetCell`). Creating a closure
  (`Closure p`) copies cell references out of the current frame's slots or
  out of the current closure's own cells, as listed in prototype `p`'s
  capture table. Captured variables are therefore shared by reference.
  Executing `InitCell` again makes a new variable, which gives Lox and
  Tiger fresh per-iteration variables in loops.

### Values

| Value | Produced by | Notes |
|---|---|---|
| nil | Lox `nil`, Monkey `null`, Tiger `nil` and unit | |
| boolean | Lox, Monkey | Tiger uses ints 0/1 |
| int | Monkey and Tiger integers | 64-bit two's complement; `+ - * /` wrap on overflow |
| float | Lox numbers | IEEE double |
| string | all three | immutable, Unicode scalar values |
| array | Monkey arrays, Tiger arrays | mutable, shared by reference |
| hash | Monkey hashes | immutable, insertion-ordered; keys are ints, booleans, strings |
| record | Tiger records | fixed fields, named by a record descriptor constant |
| closure, native | functions | |
| class, instance, bound method | Lox classes | |
| quote | Monkey `quote(...)` | the quoted syntax tree, held as its text |
| *undefined* | uninitialised slots, `GetGlobalOpt` | internal; only `JumpIfNotUndef` inspects it |

An int and a float mix in arithmetic and comparison by converting the int
to a float. *Truthiness* (used by `JumpIfFalse`, `JumpIfTrue` and `Not`)
treats nil, false and undefined as false and everything else as true.
*Zero-ness* (used by `JumpIfZero`) is defined for ints, floats and booleans.

### Equality (`Eq`, `Ne`)

Numbers compare by value, using IEEE semantics (so NaN ≠ NaN). Strings
compare by content, and booleans and nil by value. Every other value
compares by identity. Values of different types are unequal.

## Instructions

Operands are unsigned 32-bit integers: `k` indexes the constant pool, `s` a
local slot, `u` a captured cell of the running closure, `t` an instruction
index in the same function, `p` the prototype table, and `n` is a count. In
the stack-effect column the top of stack is on the right.

### Stack and constants

| Op | Effect | Meaning |
|---|---|---|
| `Const k` | → v | push int/float/string constant `k` |
| `Nil` / `True` / `False` | → v | |
| `Pop` | v → | |
| `Dup` | v → v v | |

### Variables

| Op | Effect | Meaning |
|---|---|---|
| `GetLocal s` / `SetLocal s` | → v / v → | plain slot access |
| `InitCell s` | v → | slot `s` := a **new** cell holding `v` |
| `GetCell s` / `SetCell s` | → v / v → | access the cell in slot `s` |
| `GetUpval u` / `SetUpval u` | → v / v → | access captured cell `u` |
| `DefineGlobal k` | v → | create or overwrite global named `k` |
| `GetGlobal k` | → v | error if undefined |
| `GetGlobalOpt k` | → v | push *undefined* if not defined |
| `SetGlobal k` | v → | error if undefined |

### Arithmetic and comparison

| Op | Effect | Meaning |
|---|---|---|
| `Add` | a b → c | numbers add (int+int stays int), strings concatenate |
| `Sub` `Mul` | a b → c | numbers |
| `Div` | a b → c | int÷int truncates toward zero (error on zero divisor); otherwise float division |
| `FDiv` | a b → c | always float division (Lox `/`) |
| `Neg` | a → b | numbers |
| `Not` | a → bool | negated truthiness |
| `Eq` `Ne` | a b → bool | see Equality |
| `Lt` `Le` `Gt` `Ge` | a b → bool | numbers only |
| `SLt` `SLe` `SGt` `SGe` | a b → bool | strings only, byte-wise lexicographic order |
| `BoolToInt` | bool → int | true→1, false→0 (Tiger comparison results) |
| `CheckInt` | v → int | int stays as is; an integral float becomes an int; anything else is an error |
| `CheckStr` | v → v | error unless `v` is a string |

### Control

| Op | Effect | Meaning |
|---|---|---|
| `Jump t` | | |
| `JumpIfFalse t` / `JumpIfTrue t` | v → | branch on truthiness |
| `JumpIfZero t` | v → | branch if `v` is 0, 0.0 or false |
| `JumpIfNotUndef t` | v → v | branch, keeping `v`, unless `v` is *undefined* |
| `Call n` | f a1..an → r | call a closure, bound method, class or native |
| `Return` | v → | return `v` to the caller |
| `Closure p` | → f | create a closure of prototype `p` |
| `Native n` | → f | push built-in function `n` |
| `Fail k` | | raise a runtime error with message `k` |
| `Print` | v → | print `v` the way Lox does, plus a newline |

`Call` semantics by callee type:

* **closure**: the argument count must equal the arity. The arguments
  become slots `0..n-1`.
* **bound method**: the receiver goes into slot 0 and the arguments follow
  it.
* **class**: makes a new instance. If the class has an `init` method, that
  method is called as a bound method and its result is the call's result.
  Otherwise the call must have no arguments, and the result is the
  instance.
* **native**: checks the arity (unless the native is variadic) and calls
  the built-in.

### Classes (Lox)

| Op | Effect | Meaning |
|---|---|---|
| `Class k` | → class | new empty class named `k` |
| `Inherit` | super class → class | copy `super`'s methods into `class`; error unless `super` is a class |
| `Method k` | class f → class | add closure `f` as method `k` |
| `GetProp k` | obj → v | instance: field `k`, else method `k` bound to the instance. Record: field `k` |
| `SetProp k` | obj v → v | instance: set field. Record: set an existing field |
| `GetSuper k` | this super → m | method `k` of class `super`, bound to `this` |

### Records and arrays (Tiger)

| Op | Effect | Meaning |
|---|---|---|
| `NewRecord k` | f1..fn → r | record with descriptor `k`, whose field count gives `n` |
| `GetField n` / `SetField n` | r → v / r v → | field `n`; error on nil |
| `NewArray` | size init → a | `size` copies of `init` (one shared value, as Tiger specifies) |
| `ArrayGet` / `ArraySet` | a i → v / a i v → | bounds-checked |

### Arrays and hashes (Monkey)

| Op | Effect | Meaning |
|---|---|---|
| `MakeArray n` | v1..vn → a | |
| `MakeHash n` | k1 v1..kn vn → h | error on an unhashable key |
| `Index` | x i → v | array element or hash entry; nil if absent; error on other types |

### Natives

| # | Name | Language | Arity |
|---|---|---|---|
| 0 | `clock` | Lox | 0 |
| 1–6 | `len`, `first`, `last`, `rest`, `push`, `puts` | Monkey | 1, 1, 1, 1, 2, variadic |
| 7–17 | `print`, `printi`, `flush`, `getchar`, `ord`, `chr`, `size`, `substring`, `concat`, `not`, `exit` | Tiger | 1, 1, 0, 0, 1, 1, 1, 3, 2, 1, 1 |
| 18, 19 | `%quote`, `%ast` | Monkey (internal: run-time `quote`) | 1, 1 |

At startup the VM binds the global `clock` to native 0, as jlox does.

## Binary format

All integers are little-endian. A string is a `u32` byte length followed by
that many bytes of UTF-8.

```
file      := "MNGR" version:u16(=1)
             nconsts:u32 const*  nprotos:u32 proto*  nmodules:u32 module*
const     := 0:u8 int:i64 | 1:u8 float:f64-bits | 2:u8 string
           | 3:u8 name:string nfields:u32 field:string*
proto     := name:string lang:u8 arity:u32 flags:u8 nlocals:u32
             ncaptures:u32 capture*  ncode:u32 instr*  display:string
capture   := 0:u8 slot:u32 | 1:u8 upval:u32
instr     := opcode:u8 [operand:u32] line:u32
module    := name:string lang:u8 entry:u32
```

`lang` is 0 for Lox, 1 for Monkey and 2 for Tiger. In `flags`, bit 0 means
slot 0 holds the receiver (`this`) and does not count towards `arity`, and
bit 1 marks a Lox initializer. `display` is the source text a function
prints as (Monkey prints functions as their source); it is empty otherwise.
Whether an instruction has an operand follows from its opcode:

```
no operand: 1 Nil  2 True  3 False  4 Pop  5 Dup  30 Add  31 Sub  32 Mul  33 Div
            34 FDiv  35 Neg  36 Not  37 Eq  38 Ne  39 Lt  40 Le  41 Gt  42 Ge
            43 SLt  44 SLe  45 SGt  46 SGe  47 BoolToInt  48 CheckInt  49 CheckStr
            66 Return  70 Print  81 Inherit  93 NewArray  94 ArrayGet  95 ArraySet
            102 Index
operand:    0 Const  10 GetLocal  11 SetLocal  12 InitCell  13 GetCell  14 SetCell
            15 GetUpval  16 SetUpval  17 DefineGlobal  18 GetGlobal  19 GetGlobalOpt
            20 SetGlobal  60 Jump  61 JumpIfFalse  62 JumpIfTrue  63 JumpIfZero
            64 JumpIfNotUndef  65 Call  67 Closure  68 Native  69 Fail  80 Class
            82 Method  83 GetProp  84 SetProp  85 GetSuper  90 NewRecord
            91 GetField  92 SetField  100 MakeArray  101 MakeHash
```

### Linking

`Program::link` concatenates programs. It shifts every constant and
prototype reference by the size of the tables that come before it and keeps
module order. Linking can therefore happen in the compiler
(`menageriec a.lox b.tig`) or at load time (`menagerie-vm a.mbc b.mbc`).

## Verification

Before running a program, the VM checks the following. Anything else the VM
assumes is checked at run time and reported as a runtime error, never as a
crash. `tests/fuzz_bytecode.py` exercises this with mutated bytecode.

* Every operand is in range and refers to a constant of the right kind:
  value constants for `Const`, strings for names and messages, record
  descriptors for `NewRecord`. Slots must be below `nlocals`, cell indices
  below the capture count, jump targets inside the function, and native
  numbers must exist.
* The capture table of every prototype that a function instantiates with
  `Closure` is valid in that function.
* A dataflow pass over reachable code finds the same operand-stack height
  at every instruction along every path. No instruction underflows the
  stack, and control never runs off the end of a function.
* Module entry functions take no arguments and capture nothing.
* The decoder rejects bad magic, unknown versions, unknown opcodes or tags,
  truncated input, trailing bytes, invalid UTF-8, and table lengths larger
  than the file.

## Runtime errors

A runtime error stops the program with exit status 70 and prints a message
in the style of the language whose function was running:

* Lox: `message` then `[line N]` (jlox's format)
* Monkey: `ERROR: message` (the book's REPL format)
* Tiger: `runtime error: message` then `[line N]`

Call depth is limited to 200,000 frames (`Stack overflow.`), and a single
array allocation to 2^26 elements.
