Top level functions 
===================

Top level functions are static functions that don't exist inside any 
class, similarly to static functions. For example, ``main`` is a top 
level function.

Function AST 
------------

annotation* 'fn' IDENTIFIER generics? ( '(' parameters ')' )? ( '->' type )? block

Internal representation 
-----------------------

- annotations: ``FnCall``
- name: ``Identifier``
- generics: ``GenericParams``
- params: ``Params``
- returns: ``Type | None``
- block: ``Block``
  
Errors 
------

Syntax 
++++++

- **ERR00200**: ``"Expected identifier after 'fn' keyword."``
- **ERR00201**: ``"Expected '<', '(', '->', or '{' after function name identifier."``
- **ERR00202**: ``"Expected '(', '->', or '{' after function generic parameters."``
- **ERR00203**: ``"Expected '->' or '{' after function parameters."``

Compile 
+++++++

- **ERR10200**: ``"Function provides an invalid override for another function of the same name in this scope (at line {other_line} with signature {other_signature})."``
- **ERR10201**: ``"Function incorrectly overrides a base class function ({signature} is different than {other_signature}".``
- **ERR10202**: ``"Function is attempting to override a base-class function which is not marked abstract or virtual."``
- **ERR10203**: ``"Specified return type differs from actual return type."``
- **WRN10200**: ``"Function name overrides a symbol from a greater scope."``
- **WRN10201**: ``"Function parameter {index} ({param_signature}) name overrides a symbol from a later scope."``