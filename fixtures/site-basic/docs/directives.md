---
title: Directives
---

# Custom-component directives

The built-in `callout` component ships with docgen and is rendered through the
same directive mechanism a project component uses.

:::callout{type=warning title="Back up first"}
This is a **block** directive. Its body is full markdown — including a
[[guide/intro|wikilink]] and a nested callout below.

:::callout{type=note}
A nested callout, rendered by the same recursive pipeline.
:::
:::

A leaf directive renders inline: see :note[a project component]{} for the
project-defined `note` component (it overrides nothing built-in; it is a fresh
component discovered from `components/note/`).

An unknown directive degrades to an inert error span rather than crashing:
:bogus[oops]{}.
