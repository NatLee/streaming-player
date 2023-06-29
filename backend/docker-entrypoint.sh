#!/bin/bash

echo "--------------- Collect Static"
python manage.py collectstatic --noinput
echo "--------------- Running server"
python manage.py runserver 0.0.0.0:8000
