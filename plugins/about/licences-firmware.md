## Notice

PaperPaw Copyright 2026 Hannes Thoene and the PaperPaw contributors

This product is licensed under the Apache License, Version 2.0. See the LICENSE file for details.

Bundled fonts (plugins/fonts-default/fonts) are licensed under the SIL Open Font License, Version 1.1:

- Literata, Copyright 2017 The Literata Project Authors (https://github.com/googlefonts/literata)

- Source Sans 3, Copyright 2010-2023 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source' (https://github.com/adobe-fonts/source-sans), bundled unmodified as SourceSans3.ttf. The Xteink readers draw the interface from bitmap packs made from it (one weight, Latin letters only, rasterised): a Modified Version under the licence, so they are named PaperPaw Sans, not with the Reserved Font Name, and are licensed under the SIL Open Font License, Version 1.1, as the font they are derived from.

The icon font (plugins/fonts-default/fonts/PaperPawIcons.ttf) is a subset of Tabler Icons, Copyright 2020-2025 Paweł Kuna (https://tabler.io/icons), licensed under the MIT License (fonts/TablerIcons-MIT.txt).

Hyphenation patterns (plugins/hyphenation/patterns, built in, and plugins/hyphenation-de/patterns, a plugin of the store) come from the hyph-utf8 project (https://github.com/hyphenation/tex-hyphen):

- German, reformed spelling, Copyright (c) 2013-2024 Stephan Hennig, Werner Lemberg, Günter Milde, Sander van Geloven, Georg Pfeiffer, Gisbert W. Selke, Tobias Wendorf, Keno Wehr, licensed under the MIT License.

- American English, Copyright (C) 1990, 2004, 2005 Gerard D.C. Kuiken, under an all-permissive license.

Their notices are in the patterns' README.md files.

The firmware of the Xteink readers also contains:

- Display initialisation sequences and waveform tables derived from the FreeInk SDK, Copyright (c) 2026 FreeInk, and the OpenX4 E-Paper Community SDK, Copyright (c) 2025 Open X4 E-Paper Contributors, both licensed under the MIT License (its text is below, with the crates').

- Battery gauge and board facts after Papyrix, Copyright (c) 2026 Pavel Liashkov, licensed under the MIT License.

- Mbed TLS (https://github.com/Mbed-TLS/mbedtls), Copyright The Mbed TLS Contributors, used under the Apache License, Version 2.0 (one of its two licences).

- The Wi-Fi, Bluetooth LE and radio libraries of ESP-IDF (https://github.com/espressif/esp-idf), Copyright Espressif Systems (Shanghai) Co., Ltd., provided as binaries for Espressif's chips and licensed as ESP-IDF states (Apache License, Version 2.0), and ESP-IDF's second-stage bootloader, licensed under the Apache License, Version 2.0.

- Mozilla's list of certificate authorities (webpki-roots), under the Community Data License Agreement Permissive 2.0, whose text is below.

Unicode data: the bidirectional and Arabic joining tables of crates/paperpaw-text (src/bidi/tables.rs, src/arabic/tables.rs, made by tools/unicode-tables.py) are derived from the Unicode Character Database, and crates/paperpaw-text/tests/data/BidiCharacterTest-part.txt is a part of its BidiCharacterTest.txt, Copyright (c) 1991-2026 Unicode, Inc., under the Unicode License v3 (https://www.unicode.org/license.txt).

Patched copies of fatfs, trouble-host and jni-macros (patches/) keep their licences; their changes are marked "Modified for PaperPaw".

The cats (the sleeping cat in plugins/sleep/pictures, the reading cats in plugins/reading-goal/pictures, the README's pictures in docs/images/readme and their dark versions) are PaperPaw artwork, generated with an AI image model (Google Nano Banana) for the project, and licensed under the Apache License, Version 2.0, as the rest of PaperPaw.

## PaperPaw: Apache License 2.0

Apache License Version 2.0, January 2004 http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

"License" shall mean the terms and conditions for use, reproduction, and distribution as defined by Sections 1 through 9 of this document.

"Licensor" shall mean the copyright owner or entity authorized by the copyright owner that is granting the License.

"Legal Entity" shall mean the union of the acting entity and all other entities that control, are controlled by, or are under common control with that entity. For the purposes of this definition, "control" means (i) the power, direct or indirect, to cause the direction or management of such entity, whether by contract or otherwise, or (ii) ownership of fifty percent (50%) or more of the outstanding shares, or (iii) beneficial ownership of such entity.

"You" (or "Your") shall mean an individual or Legal Entity exercising permissions granted by this License.

"Source" form shall mean the preferred form for making modifications, including but not limited to software source code, documentation source, and configuration files.

"Object" form shall mean any form resulting from mechanical transformation or translation of a Source form, including but not limited to compiled object code, generated documentation, and conversions to other media types.

"Work" shall mean the work of authorship, whether in Source or Object form, made available under the License, as indicated by a copyright notice that is included in or attached to the work (an example is provided in the Appendix below).

"Derivative Works" shall mean any work, whether in Source or Object form, that is based on (or derived from) the Work and for which the editorial revisions, annotations, elaborations, or other modifications represent, as a whole, an original work of authorship. For the purposes of this License, Derivative Works shall not include works that remain separable from, or merely link (or bind by name) to the interfaces of, the Work and Derivative Works thereof.

"Contribution" shall mean any work of authorship, including the original version of the Work and any modifications or additions to that Work or Derivative Works thereof, that is intentionally submitted to Licensor for inclusion in the Work by the copyright owner or by an individual or Legal Entity authorized to submit on behalf of the copyright owner. For the purposes of this definition, "submitted" means any form of electronic, verbal, or written communication sent to the Licensor or its representatives, including but not limited to communication on electronic mailing lists, source code control systems, and issue tracking systems that are managed by, or on behalf of, the Licensor for the purpose of discussing and improving the Work, but excluding communication that is conspicuously marked or otherwise designated in writing by the copyright owner as "Not a Contribution."

"Contributor" shall mean Licensor and any individual or Legal Entity on behalf of whom a Contribution has been received by Licensor and subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable (except as stated in this section) patent license to make, have made, use, offer to sell, sell, import, and otherwise transfer the Work, where such license applies only to those patent claims licensable by such Contributor that are necessarily infringed by their Contribution(s) alone or by combination of their Contribution(s) with the Work to which such Contribution(s) was submitted. If You institute patent litigation against any entity (including a cross-claim or counterclaim in a lawsuit) alleging that the Work or a Contribution incorporated within the Work constitutes direct or contributory patent infringement, then any patent licenses granted to You under this License for that Work shall terminate as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the Work or Derivative Works thereof in any medium, with or without modifications, and in Source or Object form, provided that You meet the following conditions:

(a) You must give any other recipients of the Work or Derivative Works a copy of this License; and

(b) You must cause any modified files to carry prominent notices stating that You changed the files; and

(c) You must retain, in the Source form of any Derivative Works that You distribute, all copyright, patent, trademark, and attribution notices from the Source form of the Work, excluding those notices that do not pertain to any part of the Derivative Works; and

(d) If the Work includes a "NOTICE" text file as part of its distribution, then any Derivative Works that You distribute must include a readable copy of the attribution notices contained within such NOTICE file, excluding those notices that do not pertain to any part of the Derivative Works, in at least one of the following places: within a NOTICE text file distributed as part of the Derivative Works; within the Source form or documentation, if provided along with the Derivative Works; or, within a display generated by the Derivative Works, if and wherever such third-party notices normally appear. The contents of the NOTICE file are for informational purposes only and do not modify the License. You may add Your own attribution notices within Derivative Works that You distribute, alongside or as an addendum to the NOTICE text from the Work, provided that such additional attribution notices cannot be construed as modifying the License.

You may add Your own copyright statement to Your modifications and may provide additional or different license terms and conditions for use, reproduction, or distribution of Your modifications, or for any such Derivative Works as a whole, provided Your use, reproduction, and distribution of the Work otherwise complies with the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise, any Contribution intentionally submitted for inclusion in the Work by You to the Licensor shall be under the terms and conditions of this License, without any additional terms or conditions. Notwithstanding the above, nothing herein shall supersede or modify the terms of any separate license agreement you may have executed with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade names, trademarks, service marks, or product names of the Licensor, except as required for reasonable and customary use in describing the origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or agreed to in writing, Licensor provides the Work (and each Contributor provides its Contributions) on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied, including, without limitation, any warranties or conditions of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE. You are solely responsible for determining the appropriateness of using or redistributing the Work and assume any risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory, whether in tort (including negligence), contract, or otherwise, unless required by applicable law (such as deliberate and grossly negligent acts) or agreed to in writing, shall any Contributor be liable to You for damages, including any direct, indirect, special, incidental, or consequential damages of any character arising as a result of this License or out of the use or inability to use the Work (including but not limited to damages for loss of goodwill, work stoppage, computer failure or malfunction, or any and all other commercial damages or losses), even if such Contributor has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing the Work or Derivative Works thereof, You may choose to offer, and charge a fee for, acceptance of support, warranty, indemnity, or other liability obligations and/or rights consistent with this License. However, in accepting such obligations, You may act only on Your own behalf and on Your sole responsibility, not on behalf of any other Contributor, and only if You agree to indemnify, defend, and hold each Contributor harmless for any liability incurred by, or claims asserted against, such Contributor by reason of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

To apply the Apache License to your work, attach the following boilerplate notice, with the fields enclosed by brackets "[]" replaced with your own identifying information. (Don't include the brackets!)  The text should be enclosed in the appropriate comment syntax for the file format. We also recommend that a file or class name and description of purpose be included on the same "printed page" as the copyright notice for easier identification within third-party archives.

Copyright 2026 Hannes Thoene

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the specific language governing permissions and limitations under the License.

## Literata: SIL Open Font License 1.1

Copyright 2017 The Literata Project Authors (https://github.com/googlefonts/literata)

This Font Software is licensed under the SIL Open Font License, Version 1.1. This license is copied below, and is also available with a FAQ at: http://scripts.sil.org/OFL

----------------------------------------------------------- SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007 -----------------------------------------------------------

PREAMBLE The goals of the Open Font License (OFL) are to stimulate worldwide development of collaborative font projects, to support the font creation efforts of academic and linguistic communities, and to provide a free and open framework in which fonts may be shared and improved in partnership with others.

The OFL allows the licensed fonts to be used, studied, modified and redistributed freely as long as they are not sold by themselves. The fonts, including any derivative works, can be bundled, embedded, redistributed and/or sold with any software provided that any reserved names are not used by derivative works. The fonts and derivatives, however, cannot be released under any other type of license. The requirement for fonts to remain under this license does not apply to any document created using the fonts or their derivatives.

DEFINITIONS "Font Software" refers to the set of files released by the Copyright Holder(s) under this license and clearly marked as such. This may include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the copyright statement(s).

"Original Version" refers to the collection of Font Software components as distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting, or substituting -- in part or in whole -- any of the components of the Original Version, by changing formats or by porting the Font Software to a new environment.

"Author" refers to any designer, engineer, programmer, technical writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS Permission is hereby granted, free of charge, to any person obtaining a copy of the Font Software, to use, study, copy, merge, embed, modify, redistribute, and sell modified and unmodified copies of the Font Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components, in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled, redistributed and/or sold with any software, provided that each copy contains the above copyright notice and this license. These can be included either as stand-alone text files, human-readable headers or in the appropriate machine-readable metadata fields within text or binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font Name(s) unless explicit written permission is granted by the corresponding Copyright Holder. This restriction only applies to the primary font name as presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font Software shall not be used to promote, endorse or advertise any Modified Version, except to acknowledge the contribution(s) of the Copyright Holder(s) and the Author(s) or with their explicit written permission.

5) The Font Software, modified or unmodified, in part or in whole, must be distributed entirely under this license, and must not be distributed under any other license. The requirement for fonts to remain under this license does not apply to any document created using the Font Software.

TERMINATION This license becomes null and void if any of the above conditions are not met.

DISCLAIMER THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT SOFTWARE.

## Source Sans 3 and PaperPaw Sans: SIL Open Font License 1.1

Copyright 2010-2023 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'. All Rights Reserved. Source is a trademark of Adobe in the United States and/or other countries.

This Font Software is licensed under the SIL Open Font License, Version 1.1.

This license is copied below, and is also available with a FAQ at: http://scripts.sil.org/OFL

----------------------------------------------------------- SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007 -----------------------------------------------------------

PREAMBLE The goals of the Open Font License (OFL) are to stimulate worldwide development of collaborative font projects, to support the font creation efforts of academic and linguistic communities, and to provide a free and open framework in which fonts may be shared and improved in partnership with others.

The OFL allows the licensed fonts to be used, studied, modified and redistributed freely as long as they are not sold by themselves. The fonts, including any derivative works, can be bundled, embedded, redistributed and/or sold with any software provided that any reserved names are not used by derivative works. The fonts and derivatives, however, cannot be released under any other type of license. The requirement for fonts to remain under this license does not apply to any document created using the fonts or their derivatives.

DEFINITIONS "Font Software" refers to the set of files released by the Copyright Holder(s) under this license and clearly marked as such. This may include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the copyright statement(s).

"Original Version" refers to the collection of Font Software components as distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting, or substituting -- in part or in whole -- any of the components of the Original Version, by changing formats or by porting the Font Software to a new environment.

"Author" refers to any designer, engineer, programmer, technical writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS Permission is hereby granted, free of charge, to any person obtaining a copy of the Font Software, to use, study, copy, merge, embed, modify, redistribute, and sell modified and unmodified copies of the Font Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components, in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled, redistributed and/or sold with any software, provided that each copy contains the above copyright notice and this license. These can be included either as stand-alone text files, human-readable headers or in the appropriate machine-readable metadata fields within text or binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font Name(s) unless explicit written permission is granted by the corresponding Copyright Holder. This restriction only applies to the primary font name as presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font Software shall not be used to promote, endorse or advertise any Modified Version, except to acknowledge the contribution(s) of the Copyright Holder(s) and the Author(s) or with their explicit written permission.

5) The Font Software, modified or unmodified, in part or in whole, must be distributed entirely under this license, and must not be distributed under any other license. The requirement for fonts to remain under this license does not apply to any document created using the Font Software.

TERMINATION This license becomes null and void if any of the above conditions are not met.

DISCLAIMER THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT SOFTWARE.

## Tabler Icons: MIT License

MIT License

Copyright (c) 2020-2025 Paweł Kuna

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## German hyphenation patterns

Copyright (c) 2013-2024 Stephan Hennig, Werner Lemberg, Günter Milde, Sander van Geloven, Georg Pfeiffer, Gisbert W. Selke, Tobias Wendorf, Keno Wehr

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## American English hyphenation patterns

Copyright (C) 1990, 2004, 2005 Gerard D.C. Kuiken

Copying and distribution of this file, with or without modification, are permitted in any medium without royalty provided the copyright notice and this notice are preserved.

## Third-party crates: MIT License

Crates in the firmware of the Xteink readers used under the MIT License:

- adler2 2.0.1: Copyright (C) Jonas Schievink <jonasschievink@gmail.com>
- aes 0.8.4: Copyright (c) 2018 Artyom Pavlov
- aligned 0.4.3: Copyright (c) 2017 Jorge Aparicio
- allocator-api2 0.3.1: Copyright Zakarum (https://github.com/zakarumych/allocator-api2)
- as-slice 0.2.1: Copyright (c) 2018 Jorge Aparicio
- base16ct 0.2.0: Copyright (c) 2014 Steve "Sc00bz" Thomas (steve at tobtu dot com); Copyright (c) 2022 The RustCrypto Project Developers
- bitfield 0.19.5: Copyright (c) 2017 Loïc Damien
- bitfield-macros 0.19.5: Copyright (c) 2017 Loïc Damien
- bitflags 1.3.2: Copyright (c) 2014 The Rust Project Developers
- bitflags 2.13.2: Copyright (c) 2014 The Rust Project Developers
- block-buffer 0.10.4: Copyright (c) 2018-2019 The RustCrypto Project Developers
- block-buffer 0.12.1: Copyright (c) 2018-2025 The RustCrypto Project Developers
- block-device-driver 0.2.0: Copyright 2024 Scott Mabin
- bt-hci 0.9.0: Copyright (c) Embassy project contributors
- bt-hci-transport 0.1.0: Copyright Embassy project contributors (https://github.com/embassy-rs/bt-hci)
- btuuid 0.1.1: Copyright Embassy project contributors (https://github.com/embassy-rs/btuuid)
- bytemuck 1.25.2: Copyright (c) 2019 Daniel "Lokathor" Gee.
- bytemuck_derive 1.12.1: Copyright (c) 2019 Daniel "Lokathor" Gee.
- byteorder 1.5.0: Copyright (c) 2015 Andrew Gallant
- cfg-if 1.0.5: Copyright (c) 2014 Alex Crichton
- cipher 0.4.4: Copyright (c) 2016-2020 RustCrypto Developers
- cmac 0.7.2: Copyright (c) 2017 Artyom Pavlov
- const-default 1.0.0: Copyright (c) 2021 by Aaron Lindsay
- const-oid 0.9.6: Copyright (c) 2020-2022 The RustCrypto Project Developers
- critical-section 1.2.0: Copyright (c) 2022 The critical-section authors
- crypto-bigint 0.5.5: Copyright (c) 2021 The RustCrypto Project Developers
- crypto-common 0.1.7: Copyright (c) 2021 RustCrypto Developers
- crypto-common 0.2.2: Copyright (c) 2021-2026 RustCrypto Developers
- darling 0.21.3: Copyright (c) 2017 Ted Driggs
- darling 0.24.1: Copyright (c) 2017 Ted Driggs
- darling_core 0.21.3: Copyright (c) 2017 Ted Driggs
- darling_core 0.24.1: Copyright (c) 2017 Ted Driggs
- darling_macro 0.21.3: Copyright (c) 2017 Ted Driggs
- darling_macro 0.24.1: Copyright (c) 2017 Ted Driggs
- dbl 0.3.2: Copyright (c) 2018-2019 The RustCrypto Project Developers
- defmt 0.3.101: Copyright The Knurling-rs developers (https://github.com/knurling-rs/defmt)
- defmt 1.2.0: Copyright (c) Ferrous Systems
- defmt-macros 1.2.0: Copyright (c) Ferrous Systems
- defmt-parser 1.1.0: Copyright (c) Ferrous Systems
- delegate 0.13.5: Copyright Godfrey Chan, Jakub Beránek (https://github.com/kobzol/rust-delegate)
- der 0.7.10: Copyright (c) 2020-2023 The RustCrypto Project Developers
- digest 0.10.7: Copyright (c) 2017 Artyom Pavlov
- digest 0.11.3: Copyright (c) 2017-2025 RustCrypto Developers; Copyright (c) 2017 Artyom Pavlov
- docsplay 0.1.3: Copyright Dániel Buga (https://github.com/bugadani/docsplay)
- docsplay-macros 0.1.2: Copyright Dániel Buga (https://github.com/bugadani/docsplay)
- document-features 0.2.12: Copyright (c) 2020 Olivier Goffart <ogoffart@sixtyfps.io>
- ed25519-compact 2.6.0: Copyright (c) 2020-2026 Frank Denis
- elliptic-curve 0.13.8: Copyright (c) 2020-2022 RustCrypto Developers
- embassy-embedded-hal 0.6.0: Copyright the embassy-embedded-hal contributors (https://github.com/embassy-rs/embassy)
- embassy-futures 0.1.2: Copyright the embassy-futures contributors (https://github.com/embassy-rs/embassy)
- embassy-hal-internal 0.4.0: Copyright the embassy-hal-internal contributors (https://github.com/embassy-rs/embassy)
- embassy-net-driver 0.2.0: Copyright the embassy-net-driver contributors (https://github.com/embassy-rs/embassy)
- embassy-sync 0.6.2: Copyright the embassy-sync contributors (https://github.com/embassy-rs/embassy)
- embassy-sync 0.7.2: Copyright the embassy-sync contributors (https://github.com/embassy-rs/embassy)
- embassy-sync 0.8.0: Copyright the embassy-sync contributors (https://github.com/embassy-rs/embassy)
- embassy-time 0.5.1: Copyright the embassy-time contributors (https://github.com/embassy-rs/embassy)
- embassy-time-driver 0.2.2: Copyright the embassy-time-driver contributors (https://github.com/embassy-rs/embassy)
- embassy-usb-driver 0.2.2: Copyright the embassy-usb-driver contributors (https://github.com/embassy-rs/embassy)
- embassy-usb-synopsys-otg 0.4.0: Copyright the embassy-usb-synopsys-otg contributors (https://github.com/embassy-rs/embassy)
- embedded-can 0.4.1: Copyright (c) 2021-2022 The Rust embedded HAL team and contributors.
- embedded-hal 0.2.7: Copyright (c) 2017-2018 Jorge Aparicio
- embedded-hal 1.0.0: Copyright (c) 2017-2018 Jorge Aparicio
- embedded-hal-async 1.0.0: Copyright (c) 2021-2022 The Rust embedded HAL team and contributors.
- embedded-hal-bus 0.3.0: Copyright (c) 2021-2022 The Rust embedded HAL team and contributors.
- embedded-io 0.6.1: Copyright (c) 2023 The embedded-io authors
- embedded-io 0.7.1: Copyright (c) 2023 The embedded-io authors
- embedded-io-async 0.6.1: Copyright (c) 2023 The embedded-io-async authors
- embedded-io-async 0.7.0: Copyright (c) 2023 The embedded-io-async authors
- embedded-storage 0.3.2: Copyright Mathias Koch (https://github.com/rust-embedded-community/embedded-storage)
- embedded-storage-async 0.4.2: Copyright (c) 2022 Diego Barrios Romero; Copyright (c) 2022 Dario Nieuwenhuis; Copyright (c) 2022 Ulf Lilleengen; Copyright (c) 2022 Mathias Koch
- enumset 1.1.14: Copyright (c) 2017-2025 Alissa Rao <aura@aura.moe>
- enumset_derive 0.15.0: Copyright (c) 2017-2025 Alissa Rao <aura@aura.moe>
- equivalent 1.0.3: Copyright (c) 2016--2023
- esp-alloc 0.11.0: Copyright the esp-alloc contributors (https://github.com/esp-rs/esp-hal)
- esp-backtrace 0.20.0: Copyright the esp-backtrace contributors (https://github.com/esp-rs/esp-hal)
- esp-bootloader-esp-idf 0.6.0: Copyright the esp-bootloader-esp-idf contributors (https://github.com/esp-rs/esp-hal)
- esp-config 0.8.0: Copyright the esp-config contributors (https://github.com/esp-rs/esp-hal)
- esp-hal 1.2.2: Copyright the esp-hal contributors (https://github.com/esp-rs/esp-hal)
- esp-hal-procmacros 0.23.0: Copyright the esp-hal-procmacros contributors (https://github.com/esp-rs/esp-hal)
- esp-metadata-generated 0.5.3: Copyright the esp-metadata-generated contributors (https://github.com/esp-rs/esp-hal)
- esp-phy 0.3.0: Copyright the esp-phy contributors (https://github.com/esp-rs/esp-hal)
- esp-println 0.18.0: Copyright the esp-println contributors (https://github.com/esp-rs/esp-hal)
- esp-radio 1.0.0-beta.1: Copyright the esp-radio contributors (https://github.com/esp-rs/esp-hal)
- esp-radio-rtos-driver 0.4.2: Copyright the esp-radio-rtos-driver contributors (https://github.com/esp-rs/esp-hal)
- esp-riscv-rt 0.15.0: Copyright the esp-riscv-rt contributors (https://github.com/esp-rs/esp-hal)
- esp-rom-sys 0.1.5: Copyright the esp-rom-sys contributors (https://github.com/esp-rs/esp-hal)
- esp-rtos 0.4.0: Copyright the esp-rtos contributors (https://github.com/esp-rs/esp-hal)
- esp-storage 0.10.0: Copyright the esp-storage contributors (https://github.com/esp-rs/esp-hal)
- esp-sync 0.3.0: Copyright the esp-sync contributors (https://github.com/esp-rs/esp-hal)
- esp-wifi-sys-esp32c3 0.3.0: Copyright the esp-wifi-sys-esp32c3 contributors (https://github.com/esp-rs/esp-wifi-sys)
- esp-wifi-sys-esp32s3 0.3.0: Copyright the esp-wifi-sys-esp32s3 contributors (https://github.com/esp-rs/esp-wifi-sys)
- esp32 0.41.0: Copyright the esp32 contributors (https://github.com/esp-rs/esp-pacs)
- esp32c2 0.30.0: Copyright the esp32c2 contributors (https://github.com/esp-rs/esp-pacs)
- esp32c3 0.33.0: Copyright the esp32c3 contributors (https://github.com/esp-rs/esp-pacs)
- esp32c3 0.34.0: Copyright the esp32c3 contributors (https://github.com/esp-rs/esp-pacs)
- esp32c5 0.3.0: Copyright the esp32c5 contributors (https://github.com/esp-rs/esp-pacs)
- esp32c6 0.24.0: Copyright the esp32c6 contributors (https://github.com/esp-rs/esp-pacs)
- esp32c61 0.4.0: Copyright the esp32c61 contributors (https://github.com/esp-rs/esp-pacs)
- esp32h2 0.20.0: Copyright the esp32h2 contributors (https://github.com/esp-rs/esp-pacs)
- esp32p4 0.3.0: Copyright the esp32p4 contributors (https://github.com/esp-rs/esp-pacs)
- esp32s2 0.32.0: Copyright the esp32s2 contributors (https://github.com/esp-rs/esp-pacs)
- esp32s3 0.36.0: Copyright the esp32s3 contributors (https://github.com/esp-rs/esp-pacs)
- esp32s3 0.37.0: Copyright the esp32s3 contributors (https://github.com/esp-rs/esp-pacs)
- esp32s31 0.3.0: Copyright the esp32s31 contributors (https://github.com/esp-rs/esp-pacs)
- fatfs 0.4.0: Copyright 2017 Rafał Harabień
- ff 0.13.1: Copyright (c) 2017 Sean Bowe
- fnv 1.0.7: Copyright (c) 2017 Contributors
- fugit 0.5.0: Copyright 2021 Emil Fresk; Copyright (c) 2021 Emil Fresk
- futures 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- futures-channel 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- futures-core 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- futures-intrusive 0.5.0: Copyright (c) 2019 Matthias Einwag
- futures-io 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- futures-sink 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- futures-task 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- futures-util 0.3.34: Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors
- gcd 2.3.0: Copyright (c) 2016 Corey Farwell
- generic-array 0.14.7: Copyright (c) 2015 Bartłomiej Kamiński
- group 0.13.0: Copyright Sean Bowe, Jack Grigg (https://github.com/zkcrypto/group)
- hash32 0.3.1: Copyright (c) 2018 Jorge Aparicio
- hashbrown 0.17.1: Copyright (c) 2016 Amanieu d'Antras
- heapless 0.8.0: Copyright (c) 2017 Jorge Aparicio
- heapless 0.9.3: Copyright (c) 2017 Jorge Aparicio
- heck 0.5.0: Copyright (c) 2015 The Rust Project Developers
- hkdf 0.12.4: Copyright (c) 2015-2018 Vlad Filippov; Copyright (c) 2018-2021 RustCrypto Developers
- hmac 0.12.1: Copyright (c) 2017 Artyom Pavlov
- hybrid-array 0.4.15: Copyright (c) 2022-2026 The RustCrypto Project Developers
- ident_case 1.0.1: Copyright Ted Driggs (https://github.com/TedDriggs/ident_case)
- indexmap 2.14.2: Copyright (c) 2016--2017
- indoc 2.0.8: Copyright David Tolnay (https://github.com/dtolnay/indoc)
- inout 0.1.4: Copyright (c) 2022 The RustCrypto Project Developers; Copyright (c) 2022 Artyom Pavlov
- instability 0.3.14: Copyright (c) 2020 Stephen M. Coakley; Copyright (c) The Ratatui Developers
- itoa 1.0.18: Copyright David Tolnay (https://github.com/dtolnay/itoa)
- linked_list_allocator 0.10.6: Copyright (c) 2016 Philipp Oppermann
- litrs 1.0.1: Copyright (c) 2020 Project Developers
- lock_api 0.4.14: Copyright (c) 2016 The Rust Project Developers
- log 0.4.34: Copyright (c) 2014 The Rust Project Developers
- mbedtls-rs 0.3.0: Copyright 2022 esp-rs; Copyright (c) 2022 esp-rs
- mbedtls-rs-sys 0.3.1: Copyright 2022 esp-rs; Copyright (c) 2022 esp-rs
- memchr 2.8.3: Copyright (c) 2015 Andrew Gallant
- miniz_oxide 0.9.1: Copyright 2013-2014 RAD Game Tools and Valve Software; Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC; Copyright (c) 2017 Frommi; Copyright (c) 2017-2024 oyvindln; Copyright (c) 2020 Frommi
- nb 0.1.3: Copyright (c) 2017 Jorge Aparicio
- nb 1.1.0: Copyright (c) 2017 Jorge Aparicio
- num-derive 0.4.2: Copyright (c) 2014 The Rust Project Developers
- num-traits 0.2.19: Copyright (c) 2014 The Rust Project Developers
- object 0.37.3: Copyright (c) 2015 The Gimli Developers
- p256 0.13.2: Copyright (c) 2020-2023 RustCrypto Developers
- paste 1.0.15: Copyright David Tolnay (https://github.com/dtolnay/paste)
- pin-project-lite 0.2.17: Copyright the pin-project-lite contributors (https://github.com/taiki-e/pin-project-lite)
- portable-atomic 1.15.0: Copyright the portable-atomic contributors (https://github.com/taiki-e/portable-atomic)
- portable_atomic_enum 0.3.1: Copyright Thomas Bächler, Dániel Buga (https://github.com/bugadani/portable_atomic_enum)
- portable_atomic_enum_macros 0.2.1: Copyright Thomas Bächler, Dániel Buga (https://github.com/bugadani/portable_atomic_enum)
- ppv-lite86 0.2.21: Copyright 2019 The CryptoCorrosion Contributors; Copyright (c) 2019 The CryptoCorrosion Contributors
- primeorder 0.13.6: Copyright (c) 2020-2023 RustCrypto Developers
- proc-macro-crate 3.5.0: Copyright Bastian Köcher (https://github.com/bkchr/proc-macro-crate)
- proc-macro2 1.0.107: Copyright David Tolnay, Alex Crichton (https://github.com/dtolnay/proc-macro2)
- quote 1.0.47: Copyright David Tolnay (https://github.com/dtolnay/quote)
- rand 0.8.8: Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers
- rand_chacha 0.3.1: Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers
- rand_core 0.10.1: Copyright (c) 2018-2026 The Rand Project Developers
- rand_core 0.6.4: Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers
- rand_core 0.9.5: Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers
- riscv-macros 0.3.0: Copyright The RISC-V Team (https://github.com/rust-embedded/riscv)
- riscv-rt-macros 0.6.1: Copyright The RISC-V Team, Jorge Aparicio (https://github.com/rust-embedded/riscv)
- rlsf 0.2.3: Copyright yvt (https://github.com/yvt/rlsf)
- rustls-pki-types 1.15.1: Copyright 2023 Dirkjan Ochtman; Copyright (c) 2023 Dirkjan Ochtman <dirkjan@ochtman.nl>
- rustversion 1.0.23: Copyright David Tolnay (https://github.com/dtolnay/rustversion)
- scopeguard 1.2.0: Copyright (c) 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers
- sdio 0.5.2: Copyright Johan Kristell, Richard Meadows (https://github.com/embassy-rs/sdio)
- sec1 0.7.3: Copyright (c) 2021-2022 The RustCrypto Project Developers
- semihosting 0.1.25: Copyright the semihosting contributors (https://github.com/taiki-e/semihosting)
- serde 1.0.229: Copyright Erick Tryzelaar, David Tolnay (https://github.com/serde-rs/serde)
- serde_core 1.0.229: Copyright Erick Tryzelaar, David Tolnay (https://github.com/serde-rs/serde)
- serde_derive 1.0.229: Copyright Erick Tryzelaar, David Tolnay (https://github.com/serde-rs/serde)
- serde_yaml 0.9.34+deprecated: Copyright David Tolnay (https://github.com/dtolnay/serde-yaml)
- sha2 0.11.1: Copyright (c) 2016-2026 The RustCrypto Project Developers; Copyright (c) 2016 Artyom Pavlov; Copyright (c) 2009-2013 Mozilla Foundation; Copyright (c) 2006-2009 Graydon Hoare
- slab 0.4.12: Copyright (c) 2019 Carl Lerche
- somni-expr 0.3.2: Copyright 2021 esp-rs; Copyright 2025 danielb
- somni-parser 0.3.1: Copyright 2021 esp-rs; Copyright 2025 danielb
- stable_deref_trait 1.2.1: Copyright (c) 2017 Robert Grosse
- static_cell 2.1.1: Copyright (c) 2022 The critical-section authors
- strsim 0.11.1: Copyright (c) 2015 Danny Guo; Copyright (c) 2016 Titus Wormer <tituswormer@gmail.com>; Copyright (c) 2018 Akash Kurdekar
- strum 0.27.2: Copyright (c) 2019 Peter Glotfelty
- strum_macros 0.27.2: Copyright (c) 2019 Peter Glotfelty
- syn 2.0.119: Copyright David Tolnay (https://github.com/dtolnay/syn)
- syn 3.0.9: Copyright David Tolnay (https://github.com/dtolnay/syn)
- termcolor 1.4.1: Copyright (c) 2015 Andrew Gallant
- thiserror 2.0.21: Copyright David Tolnay (https://github.com/dtolnay/thiserror)
- thiserror-impl 2.0.21: Copyright David Tolnay (https://github.com/dtolnay/thiserror)
- toml_datetime 1.1.2+spec-1.1.0: Copyright Individual contributors; Copyright (c) Individual contributors
- toml_edit 0.25.17+spec-1.1.0: Copyright Individual contributors; Copyright (c) Individual contributors
- toml_parser 1.1.5+spec-1.1.0: Copyright Individual contributors; Copyright (c) Individual contributors
- trouble-host 0.7.0: Copyright (c) Embassy project contributors
- typenum 1.20.1: Copyright 2014 Paho Lurie-Gregg; Copyright (c) 2014 Paho Lurie-Gregg
- ufmt-write 0.1.0: Copyright Jorge Aparicio (https://github.com/japaric/ufmt)
- unicode-ident 1.0.27: COPYRIGHT AND PERMISSION NOTICE; Copyright © 1991-2023 Unicode, Inc.
- unsafe-libyaml 0.2.11: Copyright David Tolnay (https://github.com/dtolnay/unsafe-libyaml)
- uuid 1.29.0: Copyright (c) 2014 The Rust Project Developers; Copyright (c) 2018 Ashley Mannix, Christopher Armstrong, Dylan DPC, Hunar Roop Kahlon
- vcell 0.1.3: Copyright (c) 2017 Jorge Aparicio
- void 1.0.2: Copyright Jonathan Reem (https://github.com/reem/rust-void.git)
- winnow 1.0.4: Copyright the winnow contributors (https://github.com/winnow-rs/winnow)
- xtensa-lx 0.13.0: Copyright the xtensa-lx contributors (https://github.com/esp-rs/esp-hal)
- xtensa-lx-rt 0.23.0: Copyright the xtensa-lx-rt contributors (https://github.com/esp-rs/esp-hal)
- xtensa-lx-rt-proc-macros 0.5.0: Copyright the xtensa-lx-rt-proc-macros contributors (https://github.com/esp-rs/esp-hal)
- zerocopy 0.8.63: Copyright 2023 The Fuchsia Authors; Copyright 2019 The Fuchsia Authors.
- zeroize 1.9.1: Copyright (c) 2018-2026 The RustCrypto Project Developers

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## Third-party crates: Apache License 2.0

Crates in the firmware of the Xteink readers used under the Apache License 2.0:

- ryu 1.0.23: Copyright David Tolnay (https://github.com/dtolnay/ryu)

Its text is in the chapter on PaperPaw's own licence above.

## Third-party crates: BSD Zero Clause License

Crates in the firmware of the Xteink readers used under the BSD Zero Clause License:

- managed 0.8.0: Copyright (C) 2017 whitequark@whitequark.org
- smoltcp 0.12.0: Copyright whitequark (https://github.com/smoltcp-rs/smoltcp.git)
- xarxa-driver 0.1.0: Copyright the xarxa-driver contributors (https://github.com/embassy-rs/xarxa.git)

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

## Third-party crates: ISC License

Crates in the firmware of the Xteink readers used under the ISC License:

- riscv 0.15.0: Copyright The RISC-V Team (https://github.com/rust-embedded/riscv)
- riscv-pac 0.2.0: Copyright The RISC-V Team (https://github.com/rust-embedded/riscv)
- riscv-rt 0.16.0: Copyright The RISC-V Team (https://github.com/rust-embedded/riscv)

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

## Third-party crates: BSD 3-Clause License

Crates in the firmware of the Xteink readers used under the BSD 3-Clause License:

- subtle 2.6.1: Copyright (c) 2016-2017 Isis Agora Lovecruft, Henry de Valence. All rights reserved.; Copyright (c) 2016-2024 Isis Agora Lovecruft. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## Third-party crates: Unicode License v3

Crates in the firmware of the Xteink readers used under the Unicode License v3:

- unicode-ident 1.0.27: COPYRIGHT AND PERMISSION NOTICE; Copyright © 1991-2023 Unicode, Inc.

UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a copy of data files and any associated documentation (the "Data Files") or software and any associated documentation (the "Software") to deal in the Data Files or Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, and/or sell copies of the Data Files or Software, and to permit persons to whom the Data Files or Software are furnished to do so, provided that either (a) this copyright and permission notice appear with all copies of the Data Files or Software, or (b) this copyright and permission notice appear in associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall not be used in advertising or otherwise to promote the sale, use or other dealings in these Data Files or Software without prior written authorization of the copyright holder.

## Third-party crates: Community Data License Agreement Permissive 2.0

Crates in the firmware of the Xteink readers used under the Community Data License Agreement Permissive 2.0:

- webpki-roots 1.0.10: Copyright the webpki-roots contributors (https://github.com/rustls/webpki-roots)

Community Data License Agreement - Permissive - Version 2.0

This is the Community Data License Agreement - Permissive, Version 2.0 (the "agreement"). Data Provider(s) and Data Recipient(s) agree as follows:

1. Provision of the Data

1.1. A Data Recipient may use, modify, and share the Data made available by Data Provider(s) under this agreement if that Data Recipient follows the terms of this agreement.

1.2. This agreement does not impose any restriction on a Data Recipient's use, modification, or sharing of any portions of the Data that are in the public domain or that may be used, modified, or shared under any other legal exception or limitation.

2. Conditions for Sharing Data

2.1. A Data Recipient may share Data, with or without modifications, so long as the Data Recipient makes available the text of this agreement with the shared Data.

3. No Restrictions on Results

3.1. This agreement does not impose any restriction or obligations with respect to the use, modification, or sharing of Results.

4. No Warranty; Limitation of Liability

4.1. All Data Recipients receive the Data subject to the following terms:

THE DATA IS PROVIDED ON AN "AS IS" BASIS, WITHOUT REPRESENTATIONS, WARRANTIES OR CONDITIONS OF ANY KIND, EITHER EXPRESS OR IMPLIED INCLUDING, WITHOUT LIMITATION, ANY WARRANTIES OR CONDITIONS OF TITLE, NON-INFRINGEMENT, MERCHANTABILITY OR FITNESS FOR A PARTICULAR PURPOSE.

NO DATA PROVIDER SHALL HAVE ANY LIABILITY FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING WITHOUT LIMITATION LOST PROFITS), HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE DATA OR RESULTS, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGES.

5. Definitions

5.1. "Data" means the material received by a Data Recipient under this agreement.

5.2. "Data Provider" means any person who is the source of Data provided under this agreement and in reliance on a Data Recipient's agreement to its terms.

5.3. "Data Recipient" means any person who receives Data directly or indirectly from a Data Provider and agrees to the terms of this agreement.

5.4. "Results" means any outcome obtained by computational analysis of Data, including for example machine learning models and models' insights.
