//! Cloud placeholders (RISKS R8, V8). A OneDrive "free up space" file keeps
//! its full size in the directory entry while its bytes live in the cloud.
//! Copied from Linux it arrives empty. These two judgments say whether a
//! file is such a placeholder and whether a read really brought it home.

pub const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: i64 = 0x40_0000;
pub const FILE_ATTRIBUTE_OFFLINE: i64 = 0x1000;

/// Do these attribute bits mark a cloud-only placeholder? The pinned bit is
/// not one of them (V8: pin bits are not detection).
pub fn is_placeholder(attributes: i64) -> bool {
    attributes & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE) != 0
}

/// One file's facts after it was read through: the placeholder bits are
/// gone, every byte was read, and bytes are allocated on the disk (unless
/// the file is empty). All three, or it is not materialized.
pub fn is_materialized(attributes: i64, length: i64, bytes_read: i64, allocated_bytes: i64) -> bool {
    if is_placeholder(attributes) || bytes_read != length {
        return false;
    }
    !(length > 0 && allocated_bytes <= 0)
}
