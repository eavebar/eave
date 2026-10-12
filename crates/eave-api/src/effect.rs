use crate::Task;

/// What [`Widget::update`](crate::Widget::update) asks for: async work, plus requests to the host.
pub struct Effect<M> {
    task: Task<M>,
    popover: Option<PopoverRequest>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverRequest {
    Open,
    Close,
}

impl<M> Effect<M> {
    pub fn none() -> Self {
        Self {
            task: Task::none(),
            popover: None,
        }
    }

    pub fn task(task: Task<M>) -> Self {
        Self {
            task,
            popover: None,
        }
    }

    /// Opens this widget's popover, under its item. Ignored if the widget has no popover.
    pub fn open_popover(mut self) -> Self {
        self.popover = Some(PopoverRequest::Open);
        self
    }

    pub fn close_popover(mut self) -> Self {
        self.popover = Some(PopoverRequest::Close);
        self
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> (Task<M>, Option<PopoverRequest>) {
        (self.task, self.popover)
    }
}

impl<M> From<Task<M>> for Effect<M> {
    fn from(task: Task<M>) -> Self {
        Self::task(task)
    }
}
