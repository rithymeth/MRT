from dataclasses import dataclass
from typing import List, Any, Optional

# Base class for all AST nodes
class Expr:
    pass

class Stmt:
    pass

@dataclass
class Binary(Expr):
    left: Expr
    operator: 'Token'
    right: Expr

@dataclass
class Grouping(Expr):
    expression: Expr

@dataclass
class Literal(Expr):
    value: Any

@dataclass
class Unary(Expr):
    operator: 'Token'
    right: Expr

@dataclass
class Variable(Expr):
    name: 'Token'

@dataclass
class Assign(Expr):
    name: 'Token'
    value: Expr

@dataclass
class Call(Expr):
    callee: Expr
    paren: 'Token'
    arguments: List[Expr]

# Statement nodes
@dataclass
class Expression(Stmt):
    expression: Expr

@dataclass
class Function(Stmt):
    name: 'Token'
    params: List['Token']
    body: List[Stmt]

@dataclass
class If(Stmt):
    condition: Expr
    then_branch: Stmt
    else_branch: Optional[Stmt]

@dataclass
class Return(Stmt):
    keyword: 'Token'
    value: Optional[Expr]

@dataclass
class While(Stmt):
    condition: Expr
    body: Stmt

@dataclass
class Block(Stmt):
    statements: List[Stmt]

@dataclass
class Print(Stmt):
    expression: Expr
