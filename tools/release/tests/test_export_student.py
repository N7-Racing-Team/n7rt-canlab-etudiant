"""Historical migrations must never erase a canonical Git checkout."""
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import export_student as E  # noqa: E402


@pytest.mark.parametrize("force", [False, True])
@pytest.mark.parametrize("marker", ["directory", "worktree", "bare"])
def test_git_checkout_is_protected_even_with_force(tmp_path, force, marker):
    target = tmp_path / "canonical"
    target.mkdir()
    if marker == "directory":
        (target / ".git").mkdir()
    elif marker == "worktree":
        (target / ".git").write_text("gitdir: ../admin/worktrees/fixture\n")
    else:
        (target / "HEAD").write_text("ref: refs/heads/main\n")
        (target / "config").write_text("[core]\n bare = true\n")
        (target / "objects").mkdir()
    assert "Git" in E.safe_target(target, tmp_path / "source", force)


@pytest.mark.parametrize("relation", ["inside", "contains"])
def test_target_cannot_erase_part_of_a_checkout_or_a_nested_repository(tmp_path, relation):
    target = tmp_path / "target"
    gitroot = target if relation == "inside" else target / "nested"
    gitroot.mkdir(parents=True)
    (gitroot / ".git").mkdir()
    if relation == "inside":
        target = gitroot / "new-directory"
    assert "Git" in E.safe_target(target, tmp_path / "source", True)


@pytest.mark.parametrize("force", [False, True])
def test_cli_refuses_real_git_target_without_touching_its_files(tmp_path, monkeypatch, force, capsys):
    git = shutil.which("git")
    if not git:
        pytest.skip("git missing")
    source, target = tmp_path / "combined", tmp_path / "canonical"
    (source / "app").mkdir(parents=True)
    (source / "app/Cargo.toml").write_text("[workspace]\n")
    (source / "restricted/tools").mkdir(parents=True)
    (source / "restricted/tools/export_instructor.py").write_text("# migration fixture\n")
    subprocess.run([git, "init", "--quiet", str(target)], check=True, capture_output=True)
    sentinel = target / "unpublished-review.md"
    sentinel.write_bytes(b"pending changes must survive\n")
    before = {p.relative_to(target).as_posix(): p.read_bytes() for p in target.rglob("*") if p.is_file()}
    monkeypatch.setattr(sys, "argv", ["export_student.py", str(target), "--source", str(source),
                                     *(["--force"] if force else [])])
    assert E.main() == 2
    assert "Git" in capsys.readouterr().err
    after = {p.relative_to(target).as_posix(): p.read_bytes() for p in target.rglob("*") if p.is_file()}
    assert after == before


def test_new_unversioned_target_is_accepted(tmp_path):
    assert E.safe_target(tmp_path / "new", tmp_path / "source", False) is None


@pytest.mark.parametrize("path", ["shared/protocols/n7rt-link/v0/golden-vectors.json",
                                 "app/crates/n7rt-link/tests/golden.rs",
                                 "app/crates/n7rt-solver/tests/golden.rs"])
def test_no_public_golden_name_exception_remains(path):
    assert E.forbidden_name(path) == "golden vectors or solution name"


def test_renamed_fixed_transport_fixture_is_refused(tmp_path):
    import json
    path = tmp_path / "transport.json"
    generated = bytes(range(19))
    path.write_text(json.dumps({"items": [{"fields": {"size": len(generated)},
                                          "wire_hex": generated.hex()}]}), encoding="utf-8")
    assert E.forbidden_content(path) == "fixed transport input/answer fixture"
