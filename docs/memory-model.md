# SimplyLang Memory Model

This document defines programmer-visible value, object, copying, mutation,
capture, lifetime, and equality semantics. These rules describe the language,
not the Rust mechanisms currently used to implement it.

## Core Model

SimplyLang has values and objects. Values are copied according to their
category. Collections behave as value-like containers with copy-on-write (COW)
mutation. Structs are identity-bearing objects. Functions capture referenced
values according to their origin. Scopes control the lifetime of bindings, but
do not invalidate values that have escaped. The runtime keeps valid escaped
values usable after their original scope ends.

No ownership, move, borrow, or lifetime syntax is required in SimplyLang.

## Value Categories

| Category | Copying | Mutation and sharing | Identity | Language lifetime |
|---|---|---|---|---|
| `Unit`, `Int`, `Float`, `Bool`, `Range` | Copies the represented value | No in-place mutation; a mutable binding may be reassigned | No object identity | While held by a binding or another value |
| `String` | Copies string content as a value | No in-place string mutation; a mutable binding may be reassigned | No object identity | While held by a binding or another value |
| `Array`, `List`, `Tuple`, `Matrix`, `Hash`, `Tree` | Copies the container value; storage may be shared internally | Value-like container semantics; supported mutation uses COW | Container storage identity is not observable | While a binding or another value holds the container |
| Struct instance | Copies a reference to the same instance | Message dispatch may mutate the shared instance | Yes; equality uses instance identity | Valid while retained by the runtime |
| Enum value | Copies the enum value reference; payload follows its own category | Enum envelope has no mutation operation; payload values retain their own semantics | Nominal declaration and variant identify its value form | Valid while retained by the runtime |
| Function or closure | Copies the same callable value | Function body is not mutated through the language; captured values follow their categories | Yes; equality uses callable identity | While retained by the runtime |
| `CsvStream` | Copies its descriptor value | It describes a source position/version; it is not a mutable collection | No object identity | While held by a binding or another value |

`Matrix` is a container value. It uses the same shared-container representation
as other sequence values, but this does not imply that every container kind has
the same mutation operations. The current indexed-write operation supports
arrays and lists; list `add`/`remove` and hash indexed writes are also supported.
Tuples and trees are read-only through the language, and matrices do not have
direct indexed writes through the current collection mutation operation.

### Runtime-only entities

Scope bindings, function metadata, struct and enum declarations, message
metadata, parsed programs, import caches, and source context are not ordinary
SimplyLang values. They have no user-visible copying or mutation rule except
where a language value refers to them indirectly, such as a function referring
to its body or an imported function retaining module values.

## Assignment: `is`

`name is expression` evaluates the expression and creates a binding to its
result. Copying an identifier into another binding follows the value category;
`is` is neither universally a deep copy nor universally a reference:

- Scalars, ranges, and strings behave as independent values.
- A collection copy behaves as a value-like container. It may share storage
  until one copy is mutated.
- A struct copy refers to the same object instance.
- An enum copy retains the enum declaration, variant, and payload values.
- A function copy refers to the same callable value.

Bindings are immutable by default. `mut` permits reassignment and the
collection mutation operations allowed for that binding; it does not deep-copy
or freeze objects reachable through the value.

## Reassignment: `->`

`name -> expression` replaces the value held by an existing mutable binding.
Reassignment is distinct from mutation of the old value.

If `a` and `b` hold COW collection values, then `a -> new_value` changes only
the binding `a`; `b` retains its previous collection value. If `a` and `b`
refer to the same struct, `a -> another_object` changes only `a`'s binding and
does not change what `b` refers to. In contrast, a message that mutates the
shared struct instance changes the object observed through both bindings.

## Mutation

### Collections

Copying a collection preserves value-like container behavior. When a supported
mutation is applied to one copy, that container separates from copies that
still represent the previous contents. The other copies retain their prior
contents.

### Structs

A struct is an object with identity. Copying its value does not create another
instance. Message dispatch can mutate the instance, and aliases observe the
updated state.

The distinction is intentional:

```text
collection copy + mutation  -> separate container state
struct alias + mutation     -> same object state
```

## Function Arguments

Arguments are evaluated into parameter bindings. Reassigning a parameter does
not reassign the caller's binding. The value category determines sharing:

- Scalars and strings provide the parameter with the represented value.
- Collections provide a value-like COW container; supported mutation in the
  parameter does not mutate a separate caller container copy.
- Structs refer to the same instance. A message that mutates the instance is
  visible to the caller, even though rebinding the parameter is local.
- Enums and functions follow their value and identity rules respectively;
  payload values inside an enum keep their own category semantics.

Parameter mutability controls the parameter binding and supported collection
mutation. It does not turn a struct instance into a separate copy.

## Return Values

Returning a value produces a result that remains usable after the function's
local scope ends:

- Scalars and strings return as values.
- Collections retain the returned container value and its COW behavior.
- Structs retain their instance identity.
- Enums retain their declaration, variant, and payload semantics.
- Functions and closures remain callable, including when returned from their
  defining function.

The programmer does not need to mark a return as a move or otherwise manage
the lifetime transition.

## Closure Capture

Capture behavior depends on where a function is defined.

### Nested functions

Nested functions capture referenced lexical values when the function is
created. The capture is a value snapshot, not a live scope binding:

- A captured scalar or string retains its creation-time value.
- A captured collection retains its creation-time container value; later
  mutation follows COW behavior.
- A captured struct retains the same object identity. Mutating that instance
  through a message remains visible through the closure; rebinding the original
  name does not replace the captured object.

Captured bindings are immutable inside the nested closure under the current
language rules. Nested closures do not share a live lexical environment.

### Executable top-level functions

Top-level functions in an executable source resolve global/module bindings when
called. They do not use nested-function creation-time snapshots for those
bindings. A later reassignment of a mutable global is therefore visible to the
function when it is invoked.

### Imported functions

Imported functions capture the relevant module values during that import
execution. Each import execution has its own module evaluator and captured
module state; only parsed program data is reused by the import cache. An
imported function can therefore outlive the module's local bindings while
continuing to use its captured values.

These three rules are distinct and intentional in Memory Model v1.

## Object Identity

A struct instance has identity independent of its field contents. Copying,
passing, returning, capturing, placing it in a collection, or storing it in an
enum payload preserves the reference to that instance; these operations do not
create a second struct instance.

`==` on structs tests whether both values refer to the same instance. It does
not recursively compare fields. Consequently, two separately constructed
instances with identical fields are unequal, while aliases of one instance are
equal even after that instance is mutated.

## Equality

The following table is the language-level equality contract:

| Category | Equality |
|---|---|
| `Unit`, `Int`, `Float`, `Bool` | Existing value equality; float behavior, including NaN behavior, follows floating-point equality |
| `String` | String content |
| `Range` | Range start, end, and step |
| `CsvStream` | Descriptor fields, including source version |
| `Array`, `List`, `Tuple`, `Matrix` | Structural equality of the same collection kind and contents |
| `Hash`, `Tree` | Structural equality of the same map kind and contents |
| Struct | Instance identity |
| Enum | Nominal declaration, variant, and payload equality |
| Function or closure | Callable identity |

COW backing-storage identity is not observable through equality. Evaluator or
arena-root identity is not itself a language equality rule. In particular,
equal enum values must not compare differently merely because they are stored
under different runtime roots.

When a value contains other values, equality applies recursively according to
each contained value's category:

- Collections compare their elements; a struct element compares by identity.
- Enums compare their payloads; a struct payload compares by identity.
- Nested collections and enum values continue applying these same rules.

Nominal declaration identity distinguishes declarations from different
modules, even if their source-level names are the same.

## Scope Lifetime

A binding's lifetime is not the same as the lifetime of the value it held.
When a scope ends, its local names are no longer accessible through that scope.
Values that escaped through a return, closure capture, collection, enum, or
other surviving value remain valid. The runtime handles this automatically;
the language exposes no manual lifetime operation.

The language contract guarantees usable escaped values. It does not promise
that every unreachable object is reclaimed immediately.

## Object Graphs and Cycles

Cycles are legal. Structs can refer to themselves or participate in graphs such
as `A -> B -> A`, including through mutable collection fields.

Cyclic values remain usable while their runtime roots remain alive; forming a
cycle does not by itself create a dangling reference. The current runtime does
not reclaim unreachable object entries individually, including entries in
cyclic graphs. This is a reclamation limitation, not a language-level
invalid-reference behavior.

Struct equality is identity-based and does not traverse struct fields, so
comparing structs does not recurse through a struct cycle. Structural equality
of collections and enums follows their contained values; a traversal reaching
a struct stops at that struct's identity comparison. This contract does not
define a graph-isomorphism comparison.

Tracing garbage collection is not required by current language semantics.
A future reclamation strategy, including a tracing collector, may change how
storage is reclaimed while preserving identity, COW, capture, return, equality,
and cycle behavior. No future collector is promised.

## Mental Model for SimplyLang Programmers

- Simple values behave like values.
- Collections behave like values that can share storage efficiently until a
  supported mutation separates the changed container.
- Structs are objects with identity. Giving one another name does not create a
  second object, and mutation through either alias changes that object.
- A closure remembers values according to whether it is nested, executable
  top-level, or imported.
- Returning a value does not make it invalid when a function ends.
- The runtime handles lifetime automatically.

## Runtime implementation mapping

The current Rust runtime implements the language contract using:

- scope bindings stored in a generational arena and owned through
  `BindingToken`s;
- evaluator-owned roots for function values, struct instances, enum values,
  and source text that may outlive a local scope;
- COW-backed collection storage;
- shared immutable parsed programs and function bodies;
- function values containing capture snapshots for nested and imported
  functions, while executable top-level functions use the evaluator's current
  global bindings.

These are implementation mechanisms, not SimplyLang syntax or language
semantics. A future runtime may replace them if it preserves the contract
above.

## Explicit Non-goals

Memory Model v1 does not introduce:

- move syntax;
- borrow syntax or borrow checking;
- lifetime annotations;
- user-visible references;
- manual memory management;
- tracing garbage collection;
- reference-count syntax;
- Rust ownership syntax;
- explicit deep-copy syntax; or
- new equality operators.

## Memory Model v1 — Locked

- Bindings contain values; copying follows the value category.
- Collections are value-like containers with COW mutation.
- Structs are identity-bearing objects; struct equality tests identity.
- Enum equality uses nominal declaration, variant, and payload equality.
- Function equality tests callable identity.
- Nested functions capture lexical value snapshots; executable top-level
  functions resolve globals at call time; imported functions capture module
  values for that import execution.
- Scope exit removes local bindings without invalidating valid escaped values.
- Cycles are legal and safe while runtime roots live; the current runtime does
  not reclaim unreachable object entries individually.

Implementation status:
The current runtime implements these semantics, including category-specific
equality and capture behavior.

GC:
Not required.

Ownership syntax:
None.

Programmer-visible memory management:
None.

Next runtime work:
Only proceed after this specification is accepted as the semantic contract.
