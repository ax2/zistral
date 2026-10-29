The Windows installer is built with [Inno Setup](https://jrsoftware.org/).

`ChineseSimplified.isl` is an unmodified copy of the official Chinese Simplified
translation from https://github.com/jrsoftware/issrc/blob/main/Files/Languages/ChineseSimplified.isl
(retrieved 2026-10-23). It is kept with the project so the installer can be built
when a runner's Inno Setup installation does not include that translation.
Original translation attribution is retained in the file. See `INNO-LICENSE.txt`
for its upstream license. Zistral's own code remains MIT licensed.

PATH detection walks segments using Unicode character indices consistently with string
copying. The native smoke test installs/upgrades in a path containing spaces and Chinese
characters so repeated installation cannot silently duplicate the owned PATH entry.
Packaging changes run a separate installer-only CI check without publishing a release.
