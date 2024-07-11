#!/bin/bash
docker exec -it player-backend-api bash -c "python manage.py makemigrations && python manage.py migrate"
