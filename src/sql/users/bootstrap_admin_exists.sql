SELECT EXISTS(SELECT 1 FROM users WHERE login_name = $1) AS "exists!"
