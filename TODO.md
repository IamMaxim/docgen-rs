## Bugs
- [x] When rapidly clicking pages in dev mode, the browser requests stutter: like some pool was exhausted and new connections are not accepted?
- [x] not found is unstyled in dev. Let's display a proper page, with 404 as the page body, so user can click sidebar entries to navigate off
- [x] Popups are off on a longer, scrollable page. Seems like they pick absolute screen position instead of relative to the link itself
- [x] Homepage doesn't enforce any dimensions on the components, so layout may be broken (see screenshot)
- [x] On larger screens, additional left/right padding is added to the entire page: left of navigation sidebar and right of the rail. That should be fixed, sidebar and rail should be "nailed" to viewport edges. Only page should have horizontal paddings.
- [x] (found via Psychoville) docs discovery walked hidden dirs + node_modules, polluting the sidebar/search/graph
 
## Gaps
- [x] Title/description fields from frontmatter are not rendered on page, unlike in ~/work/docgen
- [x] /index.md notes don't behave like "folder notes": they are just nested inside the folder in sidebar. They should be actual "folder notes", focused by clicking on folder
-

## Features
- [x] Let's add sidebar collapse state saving to browser local storage

## Style desires
- [x] Let's remove header dots. They clutter the design a bit and break typographic left alignment.

## Env setup
- [x] Let's add docgen config to ./fixtures and to ~/work/Psychoville

