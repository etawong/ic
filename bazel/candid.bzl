"""
This module defines functions for checking backward compatibility of candid interfaces.
"""

def _did_git_test_impl(ctx):
    check_did = ctx.executable._check_did
    script = """
set -xeuo pipefail

readonly merge_base=${{CI_MERGE_REQUEST_DIFF_BASE_SHA:-HEAD}}
readonly tmpfile=$(mktemp $TEST_TMPDIR/prev.XXXXXX)
git show $merge_base:{did_path} > $tmpfile

echo MERGE_BASE=$merge_base
echo DID_PATH={did_path}

{check_did} {did_path} "$tmpfile"
    """.format(check_did = check_did.short_path, did_path = ctx.file.did.path)

    ctx.actions.write(output = ctx.outputs.executable, content = script)

    files = depset(direct = [check_did, ctx.file.did, ctx.file._git])
    runfiles = ctx.runfiles(files = files.to_list())

    return [
        DefaultInfo(runfiles = runfiles),
        RunEnvironmentInfo(inherited_environment = ["CI_MERGE_REQUEST_DIFF_BASE_SHA"]),
    ]

CHECK_DID = attr.label(
    default = Label("//rs/tools/check_did"),
    executable = True,
    allow_single_file = True,
    cfg = "exec",
)

_did_git_test = rule(
    implementation = _did_git_test_impl,
    attrs = {
        "did": attr.label(allow_single_file = True),
        "_check_did": CHECK_DID,
        "_git": attr.label(allow_single_file = True, default = "//:.git"),
    },
    test = True,
)

def did_git_test(name, did, **kwargs):
    """Defines a test checking whether a Candid interface evolves in a backward-compatible way.

    Args:
      name: the test name.
      did: the Candid file, must be a repository file.
      **kwargs: additional keyword arguments to pass to the test rule.
    """
    kwargs.setdefault("tags", ["local", "no-sandbox", "smoke"])
    _did_git_test(name = name, did = did, **kwargs)
