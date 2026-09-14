# Windows preference publication

2026-09-14, native Windows/NTFS, Rust 1.95.0. Only synthetic temporary files.

The WP06 integration run exposed an intermittent `NotFound` (Win32 error 2)
while reading preferences during replacement. Root reproduced the original
regression on repetition 14. Returning default preferences for that window would
also be wrong, even if JSON parsing succeeded.

The corrected writer uses `MoveFileExW` with replace-existing for one
same-directory rename after flushing and closing its temporary file. It does not
copy/delete across volumes or remove the old file before publication. Sharing
and delete-pending failures have a bounded retry window; other failures propagate.

Microsoft describes [ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew)
as combining multiple operations and documents partial-failure states. Its
write-through flag is unsupported. The replacement now uses the documented
[MoveFileExW flags](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw).
This is process-concurrency evidence, not a sudden-power-loss test.

The strengthened regression starts with non-default preferences and performs
3,000 reads while a writer changes themes 50 times. Every read must retain a
sentinel target and a published theme, so a missing/default snapshot cannot pass.
It passed 30 consecutive native runs after the correction. A separate Windows
test holds the destination without delete sharing and checks bounded failure,
preserved previous settings and removal of the temporary file.
