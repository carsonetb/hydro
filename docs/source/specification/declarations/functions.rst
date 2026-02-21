Top level functions 
===================

Top level functions are static functions that don't exist inside any 
class, similarly to static functions. For example, ``main`` is a top 
level function.

Function AST 
------------

annotation* 'fn' IDENTIFIER generics? ( '(' parameters ')' )? ( '->' type )? '=' block

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

- ERR00200