#!/bin/bash
docker exec -it player-backend-api-service bash -c "python manage.py makemigrations && python manage.py migrate"
