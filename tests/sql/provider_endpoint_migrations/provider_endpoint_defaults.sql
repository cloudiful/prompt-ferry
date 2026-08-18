SELECT
    (SELECT column_default::TEXT
       FROM information_schema.columns
      WHERE table_schema = current_schema()
        AND table_name = 'provider_endpoints'
        AND column_name = 'native_api') AS "native_api_default!",
    (SELECT column_default::TEXT
       FROM information_schema.columns
      WHERE table_schema = current_schema()
         AND table_name = 'provider_endpoints'
         AND column_name = 'native_api_source') AS "native_api_source_default!"
    ,(SELECT column_default::TEXT
        FROM information_schema.columns
       WHERE table_schema = current_schema()
         AND table_name = 'provider_endpoints'
         AND column_name = 'provider') AS "provider_default!"
