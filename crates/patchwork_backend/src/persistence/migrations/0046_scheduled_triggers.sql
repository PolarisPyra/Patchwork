-- What starts a scheduled task, as JSON; NULL runs it on its schedule.
ALTER TABLE scheduled_tasks ADD COLUMN run_on TEXT;
