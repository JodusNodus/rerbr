# Installing from the disc

For players on macOS or Linux with no Windows install of Richard Burns Rally. The launcher's **Set up from the
disc…** (next to the game folder's **Browse…**) turns the disc into a game folder: the retail game with NGP, laid
out as the Windows installer would leave it. It downloads NGP 7.4.778 from gvrc.de unless you give it your copy.

## On the command line

```sh
rerbr --install-from-disc DISC NGP OUT
```

DISC is the disc's `.iso` or the disc as a folder, NGP the package's `.7z` or folder (or `-` to download it), OUT a
new or empty folder. For example:

```sh
rerbr --install-from-disc ~/Downloads/"Richard Burns rally.iso" \
    ~/Downloads/RBR_NGP-7.4.778.507.7z ~/Games/"Richard Burns Rally"
```

It prints its steps and what OUT holds. Then choose OUT as the game folder in the launcher.

## What it does

The downloads are only read. The work happens in a temporary folder next to OUT, which becomes OUT at the end. After
an error or Cancel it is removed, so OUT is untouched unless the setup completes.

What OUT gets:
- the retail game files from the disc's installer, where the installer puts them: its `Root` files at the top, the
  other content folders (`Maps/`, `Cars/`, `Textures/` and the rest) under their own names; of the manuals, the
  English one only. The installer's own support files are skipped;
- NGP's files: `Plugins/` and `AudioFMOD/`, and its `physics.rbz` in place of the retail one. NGP's package has no
  cars; the launcher offers gvrc.de's car list until the game has some.

The folder is meant for ReRBR: it holds the retail 1.0 game files, so it does not run as the original game under
Windows/Wine without the 1.02 patch.

The launcher reads the disc image, the installers' cabinets and NGP's `.7z` itself. It first checks a few
known files in each download and says which one is missing, so a wrong download stops before anything is written.
It was tested with one disc release (the English, French and German one); others may differ. Only the InstallShield
cabinet versions of this disc (6 and later) are read.
