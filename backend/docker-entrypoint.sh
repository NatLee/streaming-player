#!/bin/bash

echo "--------------- Starting entrypoint.sh"
python manage.py makemigrations && python manage.py migrate

echo "--------------- Collect Static"
python manage.py collectstatic --noinput

/usr/local/bin/supervisord -c /etc/supervisor/conf.d/supervisord.conf

