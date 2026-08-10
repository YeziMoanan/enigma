use sonettobuf::CmdId;

#[derive(Clone, Copy)]
pub enum DownTag {
    /// Allocate the next tag in the connection's writer task.
    Next,
    /// Preserve the protocol's fixed tag for packets that opt out of sequencing.
    Fixed(u8),
}

#[derive(Clone)]
pub enum CommandPacket {
    Disconnect,
    Reply {
        cmd_id: CmdId,
        body: Vec<u8>,
        result_code: i16,
        up_tag: u8,
        down_tag: DownTag,
    },
    Push {
        cmd_id: CmdId,
        body: Vec<u8>,
        down_tag: DownTag,
    },
}
