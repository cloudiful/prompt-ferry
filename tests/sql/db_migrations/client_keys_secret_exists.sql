SELECT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'client_keys'
      AND column_name = 'secret'
) AS exists;
