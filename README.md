[![Project Status: Concept – Minimal or no implementation has been done yet, or the repository is only intended to be a limited example, demo, or proof-of-concept.](https://www.repostatus.org/badges/latest/concept.svg)](https://www.repostatus.org/#concept)
[![CI Status](https://github.com/jwodder/gamdam-rust/actions/workflows/test.yml/badge.svg)](https://github.com/jwodder/gamdam-rust/actions/workflows/test.yml)
[![codecov.io](https://codecov.io/gh/jwodder/gamdam-rust/branch/master/graph/badge.svg)](https://codecov.io/gh/jwodder/gamdam-rust)
[![MIT License](https://img.shields.io/github/license/jwodder/gamdam-rust.svg)](https://opensource.org/licenses/MIT)

`gamdam` is the Git-Annex Mass Downloader and Metadata-er (in Rust!).  It takes
a stream of JSON Lines describing what files to download and what metadata each
file has, downloads them in parallel to a
[git-annex](https://git-annex.branchable.com) repository, attaches the metadata
using git-annex's metadata facilities, and commits the results.

`gamdam` requires `git-annex` v10.20220222 or higher to be installed separately
in order to run.


Usage
=====

    gamdam [<options>] [<input-file>]

`gamdam` reads a series of JSON entries from a file (or from standard input if
no file is specified) following the [input format](#input-format) described
below.  It feeds the URLs and output paths to `git-annex addurl`, and once each
file has finished downloading, it attaches any listed metadata and extra URLs
using `git-annex metadata` and `git-annex registerurl`, respectively.

Note that the latter step can only be performed on files tracked by git-annex;
if you, say, have configured git-annex to not track text files, then any text
files downloaded will not have any metadata or alternative URLs registered.

Options
-------

- `--addurl-opts <OPTIONS>` — Extra options to pass to the `git-annex addurl`
  command.  Multiple options & arguments need to be quoted as a single string,
  which must also use proper shell quoting internally, e.g.,
  `--addurl-opts="--user-agent 'gamdam via git-annex'"`.

- `-C <DIR>`, `--chdir <DIR>` — The directory in which to download files;
  defaults to the current directory.  If the directory does not exist, it will
  be created.  If the directory does not belong to a Git or git-annex
  repository, it will be initialized as one.

- `-F <FILE>`, `--failures FILE` — If any files fail to download or fail to
  have their metadata/URLs set, write their input records back out to `FILE`.

- `-J <INT>`, `--jobs <INT>` — Number of parallel jobs for `git-annex addurl`
  to use; by default, the process is instructed to use one job per CPU core.

- `-l <LEVEL>`, `--log-level <LEVEL>` — Set the log level to the given value.
  Possible values are "`OFF`", "`ERROR`", "`WARN`", "`INFO`", "`DEBUG`", and
  "`TRACE`" (all case-insensitive) [default: `INFO`]

- `-m <TEXT>`, `--message <TEXT>` — The commit message to use when saving.
  This may contain a `{downloaded}` placeholder which will be replaced with the
  number of files successfully downloaded.

- `--no-save-on-fail` — Don't commit the downloaded files if any files failed
  to download

- `--save`, `--no-save` — Whether to commit the downloaded files once they've
  all been downloaded  [default: `--save`]


Input Format
------------

Input is a series of JSON objects, one per line (a.k.a. "JSON Lines").  Each
object has the following fields:

- `url` — *(required)* A URL to download

- `path` — *(required)* A relative path where the contents of the URL should be
  saved.  If an entry with a given path is encountered while another entry with
  the same path is being downloaded, the later entry is discarded, and a
  warning is emitted.

  If a file already exists at a given path, `git-annex` will try to register
  the URL as an additional location for the file, failing if the resource at
  the URL is not the same size as the extant file.

  Paths must be relative to the directory specified with `--chdir`, cannot
  contain "`..`" as a path component, cannot end with a path separator, and
  cannot be empty or contain only the path component "`.`".  Forward slashes
  (`/`) are accepted as path separators on all platforms, while backslashes are
  only treated as path separators on Windows.

- `metadata` — A collection of metadata in the form used by `git-annex
  metadata --json`, i.e., a mapping of key names to lists of string values.

- `extra_urls` — A list of alternative URLs for the resource, to be attached to
  the downloaded file with `git-annex registerurl`.

If a given input line is invalid, it is discarded, and a warning message is
emitted.
