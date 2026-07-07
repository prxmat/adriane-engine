# Adriane Elixir SDK

Elixir wrapper over `adriane-c-api` through a small C NIF.

Build the NIF against Erlang headers and link it with `adriane_c_api`, then place
the produced shared object at `priv/adriane_nif`.
