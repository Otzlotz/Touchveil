use crate::errors::DomainError;

pub trait ShellSystemPort {
    fn run(&mut self) -> Result<(), DomainError>;
}

pub trait ShellContextPort {
    fn render_frame_if_dirty(&mut self) -> Result<(), DomainError>;
}
