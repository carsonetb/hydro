Class declarations 
==================

All classes inherit ``Object``. When a class is defined, a global 
constant of type ``Type`` is created with the same name as the class.
This constant has a ``()`` operator which can be used to instantiate 
the class. 

Class AST 
---------

.. code-block:: 

    generics_def = ':' ( type | ( '(' type ( ',' type )* ')' ) )
    members = declaration ( ',' declaration )*
    class = annotation* 'class' IDENTIFIER generics_def? inheritance? ( '(' parameters? ')' )? '=' ( '(' members? ')' )