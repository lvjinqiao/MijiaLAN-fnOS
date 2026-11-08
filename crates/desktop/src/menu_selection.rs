//! Convert native menu command IDs (one-based; zero means cancelled).

pub fn selected_index(command: i32, option_count: usize) -> Option<usize> {
    let index = usize::try_from(command).ok()?.checked_sub(1)?;
    (index < option_count).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::selected_index;

    #[test]
    fn cancellation_and_invalid_commands_have_no_selection() {
        for command in [0, -1, i32::MIN, 7, i32::MAX] {
            assert_eq!(selected_index(command, 6), None, "command {command}");
        }
        assert_eq!(selected_index(1, 0), None);
    }

    #[test]
    fn command_ids_select_the_first_middle_and_last_option() {
        assert_eq!(selected_index(1, 6), Some(0));
        assert_eq!(selected_index(3, 6), Some(2));
        assert_eq!(selected_index(6, 6), Some(5));
    }
}
